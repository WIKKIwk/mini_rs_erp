-- Establish the append-only canonical revision authority and its materialized
-- runtime projections. Existing 0065-0068 rows remain visible only as legacy
-- migration input until the exact P11 cutover; a row is canonical at runtime
-- only when source_revision/source_aasx_sha256 are present and aligned to head.

CREATE TABLE mini_canonical_apparatus_identities (
    apparatus_id TEXT PRIMARY KEY,
    physical_asset_id TEXT NOT NULL UNIQUE,
    aas_shell_id TEXT NOT NULL UNIQUE,
    aas_submodel_id TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_canonical_apparatus_identity_id_shape CHECK (
        apparatus_id = btrim(apparatus_id)
        AND octet_length(apparatus_id) <= 128
        AND apparatus_id !~ '[[:space:][:cntrl:]]'
        AND apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
    ),
    CONSTRAINT mini_canonical_apparatus_physical_asset_not_blank CHECK (
        btrim(physical_asset_id) <> ''
        AND physical_asset_id = btrim(physical_asset_id)
        AND physical_asset_id !~ '[[:space:][:cntrl:]]'
    ),
    CONSTRAINT mini_canonical_apparatus_aas_shell_not_blank CHECK (
        btrim(aas_shell_id) <> '' AND aas_shell_id = btrim(aas_shell_id)
    ),
    CONSTRAINT mini_canonical_apparatus_aas_submodel_not_blank CHECK (
        btrim(aas_submodel_id) <> '' AND aas_submodel_id = btrim(aas_submodel_id)
    ),
    CONSTRAINT mini_canonical_apparatus_identity_composite_unique UNIQUE (
        apparatus_id, physical_asset_id
    )
);

CREATE TABLE mini_canonical_apparatus_revisions (
    apparatus_id TEXT NOT NULL,
    revision BIGINT NOT NULL,
    schema_version INTEGER NOT NULL,
    canonical_payload JSONB NOT NULL,
    aasx_package BYTEA NOT NULL,
    aasx_sha256 TEXT NOT NULL,
    equipment_class_id TEXT NOT NULL,
    physical_asset_id TEXT NOT NULL,
    aas_shell_id TEXT NOT NULL,
    aas_submodel_id TEXT NOT NULL,
    aas_semantic_id TEXT NOT NULL,
    lifecycle_state TEXT NOT NULL,
    committed_at_unix_ms BIGINT NOT NULL,
    actor_id TEXT NOT NULL,
    command_id TEXT NOT NULL,
    revision_source TEXT NOT NULL,
    source_reference TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (apparatus_id, revision),
    CONSTRAINT mini_canonical_apparatus_revision_identity_fk
        FOREIGN KEY (apparatus_id, physical_asset_id)
        REFERENCES mini_canonical_apparatus_identities (apparatus_id, physical_asset_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT mini_canonical_apparatus_revision_positive CHECK (revision > 0),
    CONSTRAINT mini_canonical_apparatus_schema_positive CHECK (schema_version > 0),
    CONSTRAINT mini_canonical_apparatus_aasx_present CHECK (octet_length(aasx_package) > 0),
    CONSTRAINT mini_canonical_apparatus_sha256_lower_hex CHECK (
        aasx_sha256 ~ '^[0-9a-f]{64}$'
    ),
    CONSTRAINT mini_canonical_apparatus_lifecycle_allowed CHECK (
        lifecycle_state IN ('active', 'retired')
    ),
    CONSTRAINT mini_canonical_apparatus_revision_source_allowed CHECK (
        revision_source IN ('admin', 'aasx_import', 'legacy_migration')
    ),
    CONSTRAINT mini_canonical_apparatus_payload_object CHECK (
        jsonb_typeof(canonical_payload) = 'object'
    ),
    CONSTRAINT mini_canonical_apparatus_payload_identity CHECK ((
        canonical_payload #>> '{apparatus_id}' = apparatus_id
        AND canonical_payload #>> '{physical_asset_id}' = physical_asset_id
        AND canonical_payload #>> '{equipment_class_id}' = equipment_class_id
        AND canonical_payload #>> '{aas_identity,shell_id}' = aas_shell_id
        AND canonical_payload #>> '{aas_identity,submodel_id}' = aas_submodel_id
        AND canonical_payload #>> '{aas_identity,semantic_id}' = aas_semantic_id
        AND canonical_payload #>> '{lifecycle,state}' = lifecycle_state
        AND (canonical_payload #>> '{schema_version}')::INTEGER = schema_version
        AND (canonical_payload #>> '{revision_metadata,revision}')::BIGINT = revision
        AND (canonical_payload #>> '{revision_metadata,committed_at_unix_ms}')::BIGINT
            = committed_at_unix_ms
        AND canonical_payload #>> '{revision_metadata,actor_id}' = actor_id
        AND canonical_payload #>> '{revision_metadata,command_id}' = command_id
        AND canonical_payload #>> '{revision_metadata,source}' = revision_source
    ) IS TRUE),
    CONSTRAINT mini_canonical_apparatus_revision_hash_unique UNIQUE (
        apparatus_id, revision, aasx_sha256
    ),
    CONSTRAINT mini_canonical_apparatus_revision_command_unique UNIQUE (command_id)
);

CREATE TABLE mini_canonical_apparatus_heads (
    apparatus_id TEXT PRIMARY KEY,
    current_revision BIGINT NOT NULL,
    current_aasx_sha256 TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_canonical_apparatus_head_revision_fk
        FOREIGN KEY (apparatus_id, current_revision, current_aasx_sha256)
        REFERENCES mini_canonical_apparatus_revisions (
            apparatus_id, revision, aasx_sha256
        )
        ON UPDATE RESTRICT ON DELETE RESTRICT
        DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE mini_canonical_apparatus_change_outbox (
    event_id TEXT PRIMARY KEY,
    apparatus_id TEXT NOT NULL,
    revision BIGINT NOT NULL,
    event_type TEXT NOT NULL,
    event_payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    published_at TIMESTAMPTZ,
    CONSTRAINT mini_canonical_apparatus_outbox_revision_fk
        FOREIGN KEY (apparatus_id, revision)
        REFERENCES mini_canonical_apparatus_revisions (apparatus_id, revision)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT mini_canonical_apparatus_outbox_type_allowed CHECK (
        event_type IN ('apparatus_created', 'apparatus_updated', 'apparatus_retired')
    ),
    CONSTRAINT mini_canonical_apparatus_outbox_payload_object CHECK (
        jsonb_typeof(event_payload) = 'object'
    ),
    CONSTRAINT mini_canonical_apparatus_one_event_per_revision UNIQUE (
        apparatus_id, revision
    )
);

-- mini_apparatus is the runtime projection. Legacy payload checks and display
-- uniqueness are retired here; display text is not identity and duplicate
-- display names are explicitly valid.
ALTER TABLE mini_apparatus
    DROP CONSTRAINT IF EXISTS mini_apparatus_name_unique,
    DROP CONSTRAINT IF EXISTS mini_apparatus_canonical_payload_contract_check,
    ADD COLUMN source_revision BIGINT,
    ADD COLUMN source_aasx_sha256 TEXT,
    ADD COLUMN schema_version INTEGER,
    ADD COLUMN physical_asset_id TEXT,
    ADD COLUMN equipment_class_id TEXT,
    ADD COLUMN hierarchy_json JSONB,
    ADD COLUMN capabilities_json JSONB,
    ADD COLUMN execution_profile_json JSONB,
    ADD COLUMN policies_json JSONB,
    ADD COLUMN capacity_json JSONB,
    ADD COLUMN lifecycle_state TEXT;

-- Display text is not identity. Older migrations expressed that obsolete
-- authority through standalone indexes rather than named table constraints.
DROP INDEX IF EXISTS idx_mini_apparatus_lower_name;
DROP INDEX IF EXISTS idx_mini_apparatus_material_rules_lower_apparatus;

ALTER TABLE mini_apparatus
    ADD CONSTRAINT mini_apparatus_runtime_revision_fk
    FOREIGN KEY (id, source_revision, source_aasx_sha256)
    REFERENCES mini_canonical_apparatus_revisions (
        apparatus_id, revision, aasx_sha256
    ) ON UPDATE RESTRICT ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT mini_apparatus_runtime_identity_fk
    FOREIGN KEY (id, physical_asset_id)
    REFERENCES mini_canonical_apparatus_identities (apparatus_id, physical_asset_id)
    ON UPDATE RESTRICT ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED,
    ADD CONSTRAINT mini_apparatus_runtime_source_complete CHECK ((
        (source_revision IS NULL AND source_aasx_sha256 IS NULL)
        OR (
            source_revision IS NOT NULL
            AND source_aasx_sha256 ~ '^[0-9a-f]{64}$'
            AND schema_version IS NOT NULL
            AND physical_asset_id IS NOT NULL
            AND equipment_class_id IS NOT NULL
            AND jsonb_typeof(hierarchy_json) = 'object'
            AND jsonb_typeof(capabilities_json) = 'object'
            AND jsonb_typeof(execution_profile_json) = 'object'
            AND jsonb_typeof(policies_json) = 'object'
            AND jsonb_typeof(capacity_json) = 'object'
            AND lifecycle_state IN ('active', 'retired')
            AND jsonb_typeof(payload_json) = 'object'
            AND payload_json #>> '{apparatus_id}' = id
            AND payload_json #>> '{display,display_name}' = name
            AND (payload_json #>> '{source_revision}')::BIGINT = source_revision
            AND payload_json #>> '{source_aasx_sha256}' = source_aasx_sha256
        )
    ) IS TRUE);

-- Optional performance projections remain derived only. Null provenance marks
-- pre-cutover legacy input; canonical rows always carry exact head provenance.
ALTER TABLE mini_apparatus_queue_policies
    ADD COLUMN source_revision BIGINT,
    ADD COLUMN source_aasx_sha256 TEXT;
ALTER TABLE mini_apparatus_material_rules
    ADD COLUMN source_revision BIGINT,
    ADD COLUMN source_aasx_sha256 TEXT;
ALTER TABLE mini_apparatus_capacity_profiles
    ADD COLUMN source_revision BIGINT,
    ADD COLUMN source_aasx_sha256 TEXT;

ALTER TABLE mini_apparatus_queue_policies
    ADD CONSTRAINT mini_apparatus_queue_projection_revision_fk
    FOREIGN KEY (canonical_apparatus_id, source_revision, source_aasx_sha256)
    REFERENCES mini_canonical_apparatus_revisions (
        apparatus_id, revision, aasx_sha256
    ) ON UPDATE RESTRICT ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE mini_apparatus_material_rules
    ADD CONSTRAINT mini_apparatus_material_projection_revision_fk
    FOREIGN KEY (canonical_apparatus_id, source_revision, source_aasx_sha256)
    REFERENCES mini_canonical_apparatus_revisions (
        apparatus_id, revision, aasx_sha256
    ) ON UPDATE RESTRICT ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE mini_apparatus_capacity_profiles
    ADD CONSTRAINT mini_apparatus_capacity_projection_revision_fk
    FOREIGN KEY (canonical_apparatus_id, source_revision, source_aasx_sha256)
    REFERENCES mini_canonical_apparatus_revisions (
        apparatus_id, revision, aasx_sha256
    ) ON UPDATE RESTRICT ON DELETE RESTRICT
    DEFERRABLE INITIALLY DEFERRED;

CREATE FUNCTION mini_require_canonical_apparatus_writer()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF current_setting('mini_rs_erp.canonical_writer', true) IS DISTINCT FROM 'on' THEN
        RAISE EXCEPTION 'canonical apparatus tables are writable only by CanonicalApparatusService'
            USING ERRCODE = '42501';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;

CREATE FUNCTION mini_reject_canonical_identity_or_revision_mutation()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'canonical apparatus identity and revision rows are append-only'
        USING ERRCODE = '55000';
END
$$;

CREATE TRIGGER mini_canonical_identity_writer_guard
BEFORE INSERT ON mini_canonical_apparatus_identities
FOR EACH ROW EXECUTE FUNCTION mini_require_canonical_apparatus_writer();
CREATE TRIGGER mini_canonical_identity_immutable
BEFORE UPDATE OR DELETE ON mini_canonical_apparatus_identities
FOR EACH ROW EXECUTE FUNCTION mini_reject_canonical_identity_or_revision_mutation();
CREATE TRIGGER mini_canonical_revision_writer_guard
BEFORE INSERT ON mini_canonical_apparatus_revisions
FOR EACH ROW EXECUTE FUNCTION mini_require_canonical_apparatus_writer();
CREATE TRIGGER mini_canonical_revision_immutable
BEFORE UPDATE OR DELETE ON mini_canonical_apparatus_revisions
FOR EACH ROW EXECUTE FUNCTION mini_reject_canonical_identity_or_revision_mutation();
CREATE TRIGGER mini_canonical_head_writer_guard
BEFORE INSERT OR UPDATE OR DELETE ON mini_canonical_apparatus_heads
FOR EACH ROW EXECUTE FUNCTION mini_require_canonical_apparatus_writer();
CREATE TRIGGER mini_canonical_outbox_writer_guard
BEFORE INSERT OR DELETE ON mini_canonical_apparatus_change_outbox
FOR EACH ROW EXECUTE FUNCTION mini_require_canonical_apparatus_writer();

CREATE FUNCTION mini_guard_canonical_outbox_update()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.event_id <> OLD.event_id
       OR NEW.apparatus_id <> OLD.apparatus_id
       OR NEW.revision <> OLD.revision
       OR NEW.event_type <> OLD.event_type
       OR NEW.event_payload <> OLD.event_payload
       OR NEW.created_at <> OLD.created_at
       OR OLD.published_at IS NOT NULL
       OR NEW.published_at IS NULL THEN
        RAISE EXCEPTION 'canonical apparatus outbox events are immutable except first publication'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER mini_canonical_outbox_update_guard
BEFORE UPDATE ON mini_canonical_apparatus_change_outbox
FOR EACH ROW EXECUTE FUNCTION mini_guard_canonical_outbox_update();

CREATE FUNCTION mini_guard_canonical_projection_write()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    old_revision BIGINT;
    new_revision BIGINT;
BEGIN
    IF TG_OP <> 'INSERT' THEN
        old_revision := OLD.source_revision;
    END IF;
    IF TG_OP <> 'DELETE' THEN
        new_revision := NEW.source_revision;
    END IF;
    IF (old_revision IS NOT NULL OR new_revision IS NOT NULL)
       AND current_setting('mini_rs_erp.canonical_writer', true) IS DISTINCT FROM 'on' THEN
        RAISE EXCEPTION 'canonical apparatus projections are derived and read-only'
            USING ERRCODE = '42501';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER mini_apparatus_projection_writer_guard
BEFORE INSERT OR UPDATE OR DELETE ON mini_apparatus
FOR EACH ROW EXECUTE FUNCTION mini_guard_canonical_projection_write();
CREATE TRIGGER mini_apparatus_queue_projection_writer_guard
BEFORE INSERT OR UPDATE OR DELETE ON mini_apparatus_queue_policies
FOR EACH ROW EXECUTE FUNCTION mini_guard_canonical_projection_write();
CREATE TRIGGER mini_apparatus_material_projection_writer_guard
BEFORE INSERT OR UPDATE OR DELETE ON mini_apparatus_material_rules
FOR EACH ROW EXECUTE FUNCTION mini_guard_canonical_projection_write();
CREATE TRIGGER mini_apparatus_capacity_projection_writer_guard
BEFORE INSERT OR UPDATE OR DELETE ON mini_apparatus_capacity_profiles
FOR EACH ROW EXECUTE FUNCTION mini_guard_canonical_projection_write();

CREATE FUNCTION mini_validate_canonical_apparatus_alignment(target_id TEXT)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    head_revision BIGINT;
    head_hash TEXT;
BEGIN
    SELECT current_revision, current_aasx_sha256
    INTO head_revision, head_hash
    FROM mini_canonical_apparatus_heads
    WHERE apparatus_id = target_id;
    IF NOT FOUND THEN
        RETURN;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM mini_apparatus
        WHERE id = target_id
          AND source_revision = head_revision
          AND source_aasx_sha256 = head_hash
    ) THEN
        RAISE EXCEPTION 'canonical apparatus head/runtime projection mismatch for %', target_id
            USING ERRCODE = '23514';
    END IF;
    IF EXISTS (
        SELECT 1 FROM mini_apparatus_queue_policies
        WHERE canonical_apparatus_id = target_id
          AND (source_revision, source_aasx_sha256)
              IS DISTINCT FROM (head_revision, head_hash)
        UNION ALL
        SELECT 1 FROM mini_apparatus_material_rules
        WHERE canonical_apparatus_id = target_id
          AND (source_revision, source_aasx_sha256)
              IS DISTINCT FROM (head_revision, head_hash)
        UNION ALL
        SELECT 1 FROM mini_apparatus_capacity_profiles
        WHERE canonical_apparatus_id = target_id
          AND (source_revision, source_aasx_sha256)
              IS DISTINCT FROM (head_revision, head_hash)
    ) THEN
        RAISE EXCEPTION 'canonical apparatus derived projection drift for %', target_id
            USING ERRCODE = '23514';
    END IF;
END
$$;

CREATE FUNCTION mini_check_canonical_apparatus_alignment()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'INSERT' THEN
        PERFORM mini_validate_canonical_apparatus_alignment(OLD.apparatus_id);
    END IF;
    IF TG_OP <> 'DELETE' THEN
        PERFORM mini_validate_canonical_apparatus_alignment(NEW.apparatus_id);
    END IF;
    RETURN NULL;
END
$$;

CREATE CONSTRAINT TRIGGER mini_canonical_head_alignment
AFTER INSERT OR UPDATE OR DELETE ON mini_canonical_apparatus_heads
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION mini_check_canonical_apparatus_alignment();

CREATE VIEW mini_canonical_apparatus_projection_drift AS
SELECT head.apparatus_id,
       head.current_revision,
       head.current_aasx_sha256,
       runtime.source_revision AS runtime_revision,
       runtime.source_aasx_sha256 AS runtime_hash,
       queue.source_revision AS queue_revision,
       queue.source_aasx_sha256 AS queue_hash,
       material.source_revision AS material_revision,
       material.source_aasx_sha256 AS material_hash,
       capacity.source_revision AS capacity_revision,
       capacity.source_aasx_sha256 AS capacity_hash
FROM mini_canonical_apparatus_heads head
LEFT JOIN mini_apparatus runtime ON runtime.id = head.apparatus_id
LEFT JOIN mini_apparatus_queue_policies queue
  ON queue.canonical_apparatus_id = head.apparatus_id
LEFT JOIN mini_apparatus_material_rules material
  ON material.canonical_apparatus_id = head.apparatus_id
LEFT JOIN mini_apparatus_capacity_profiles capacity
  ON capacity.canonical_apparatus_id = head.apparatus_id
WHERE (runtime.source_revision, runtime.source_aasx_sha256)
        IS DISTINCT FROM (head.current_revision, head.current_aasx_sha256)
   OR (queue.canonical_apparatus_id IS NOT NULL AND
       (queue.source_revision, queue.source_aasx_sha256)
        IS DISTINCT FROM (head.current_revision, head.current_aasx_sha256))
   OR (material.canonical_apparatus_id IS NOT NULL AND
       (material.source_revision, material.source_aasx_sha256)
        IS DISTINCT FROM (head.current_revision, head.current_aasx_sha256))
   OR (capacity.canonical_apparatus_id IS NOT NULL AND
       (capacity.source_revision, capacity.source_aasx_sha256)
        IS DISTINCT FROM (head.current_revision, head.current_aasx_sha256));

CREATE INDEX idx_mini_canonical_apparatus_revisions_created
    ON mini_canonical_apparatus_revisions (apparatus_id, revision DESC);
CREATE INDEX idx_mini_canonical_apparatus_outbox_unpublished
    ON mini_canonical_apparatus_change_outbox (created_at, event_id)
    WHERE published_at IS NULL;
