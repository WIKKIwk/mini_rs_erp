-- Qolip identity can travel downstream as production lineage without making
-- every downstream apparatus session an owner of the physical tooling lock.
-- Backfill only sessions running on a canonical apparatus whose own tooling
-- policy requires the Qolip scan.

UPDATE mini_order_run_sessions AS session
SET payload_json = jsonb_set(
    session.payload_json,
    '{qolip_lock_owner}',
    'true'::jsonb,
    true
)
FROM mini_apparatus AS apparatus
WHERE apparatus.id = session.canonical_apparatus_id
  AND apparatus.policies_json #>> '{tooling,mode}' = 'qolip_scan_required'
  AND (
      btrim(COALESCE(session.payload_json->>'qolip_code', '')) <> ''
      OR jsonb_array_length(
          CASE
              WHEN jsonb_typeof(session.payload_json->'qolip_codes') = 'array'
              THEN session.payload_json->'qolip_codes'
              ELSE '[]'::jsonb
          END
      ) > 0
  );

ALTER TABLE mini_order_run_sessions
    ADD CONSTRAINT mini_order_run_sessions_qolip_lock_owner_boolean CHECK (
        payload_json->'qolip_lock_owner' IS NULL
        OR jsonb_typeof(payload_json->'qolip_lock_owner') = 'boolean'
    ),
    ADD CONSTRAINT mini_order_run_sessions_qolip_lock_owner_has_identity CHECK (
        payload_json->>'qolip_lock_owner' IS DISTINCT FROM 'true'
        OR (
            btrim(COALESCE(payload_json->>'qolip_code', '')) <> ''
            OR jsonb_array_length(
                CASE
                    WHEN jsonb_typeof(payload_json->'qolip_codes') = 'array'
                    THEN payload_json->'qolip_codes'
                    ELSE '[]'::jsonb
                END
            ) > 0
        )
    );
