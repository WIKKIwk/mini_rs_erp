UPDATE mini_progress_batches
SET payload_json = payload_json - array[
    'status_detail',
    'wip_status',
    'current_apparatus',
    'current_apparatus_key',
    'current_location',
    'next_apparatus',
    'parent_batch_id',
    'used_by_session_id',
    'used_by_apparatus',
    'used_by_order_id',
    'processed_by_session_id',
    'processed_by_apparatus',
    'from_apparatus'
]::text[]
WHERE payload_json ?| array[
    'status_detail',
    'wip_status',
    'current_apparatus',
    'current_apparatus_key',
    'current_location',
    'next_apparatus',
    'parent_batch_id',
    'used_by_session_id',
    'used_by_apparatus',
    'used_by_order_id',
    'processed_by_session_id',
    'processed_by_apparatus',
    'from_apparatus'
]::text[];

ALTER TABLE mini_progress_batches
    DROP CONSTRAINT IF EXISTS mini_progress_batches_wip_typed_payload_forbidden;
ALTER TABLE mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_wip_typed_payload_forbidden
    CHECK (NOT (payload_json ?| array[
        'status_detail',
        'wip_status',
        'current_apparatus',
        'current_apparatus_key',
        'current_location',
        'next_apparatus',
        'parent_batch_id',
        'used_by_session_id',
        'used_by_apparatus',
        'used_by_order_id',
        'processed_by_session_id',
        'processed_by_apparatus',
        'from_apparatus'
    ]::text[]));
