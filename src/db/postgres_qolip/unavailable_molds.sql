SELECT DISTINCT ON (code_key) code_key, reason
FROM (
    SELECT code_key, 'qolip_archived' AS reason, 1 AS priority
    FROM mini_qolip_archive_entries
    WHERE kind = 'qolip' AND restored_at IS NULL
    UNION ALL
    SELECT member.qolip_code_key,
        CASE WHEN paddon.archived_at IS NULL THEN 'qolip_in_paddon' ELSE 'qolip_archived' END,
        CASE WHEN paddon.archived_at IS NULL THEN 2 ELSE 1 END
    FROM mini_qolip_paddon_members member
    JOIN mini_qolip_paddons paddon ON paddon.code = member.paddon_code
    WHERE member.removed_at IS NULL
    UNION ALL
    SELECT qolip_code_key, 'qolip_in_rework', 3
    FROM mini_qolip_reworks
    WHERE returned_at IS NULL
    UNION ALL
    SELECT lower(btrim(qolip_code)), 'qolip_in_transfer', 4
    FROM mini_qolip_locations
    WHERE btrim(inventory_transfer_id) <> ''
    UNION ALL
    SELECT lower(btrim(qolip_code)), 'qolip_in_use', 5
    FROM mini_qolip_checkouts
    WHERE lower(status) = 'open'
    UNION ALL
    SELECT lower(btrim(mold.code)), 'qolip_in_use', 5
    FROM mini_order_run_sessions session
    CROSS JOIN LATERAL (
        SELECT session.payload_json ->> 'qolip_code' AS code
        UNION ALL
        SELECT jsonb_array_elements_text(
            CASE WHEN jsonb_typeof(session.payload_json -> 'qolip_codes') = 'array'
                 THEN session.payload_json -> 'qolip_codes' ELSE '[]'::jsonb END
        )
    ) mold
    WHERE session.status IN ('active', 'paused', 'frozen', 'roll_detached')
      AND session.payload_json ->> 'qolip_lock_owner' = 'true'
      AND btrim(mold.code) <> ''
) holds
ORDER BY code_key, priority
