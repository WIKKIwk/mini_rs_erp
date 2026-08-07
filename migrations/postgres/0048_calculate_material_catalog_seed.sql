INSERT INTO mini_calculate_materials (id, lower_name, payload_json, updated_at)
VALUES
    (
        'builtin-bopp',
        'bopp',
        '{"id":"builtin-bopp","name":"BOPP","active":true,"density_g_cm3":0.91,"variants":[{"micron":18},{"micron":20},{"micron":25},{"micron":30},{"micron":35},{"micron":40},{"micron":45},{"micron":50},{"micron":60}]}'::jsonb,
        now()
    ),
    (
        'builtin-mcpp',
        'mcpp',
        '{"id":"builtin-mcpp","name":"MCPP","active":true,"density_g_cm3":0.90,"variants":[{"micron":20},{"micron":25},{"micron":30},{"micron":35},{"micron":40},{"micron":45},{"micron":50},{"micron":60}]}'::jsonb,
        now()
    ),
    (
        'builtin-pe-oq',
        'pe oq',
        '{"id":"builtin-pe-oq","name":"PE oq","active":true,"density_g_cm3":0.92,"variants":[{"micron":30},{"micron":35},{"micron":40},{"micron":45},{"micron":50},{"micron":55},{"micron":60},{"micron":65},{"micron":70},{"micron":75},{"micron":80},{"micron":85},{"micron":90}]}'::jsonb,
        now()
    ),
    (
        'builtin-pe-qora',
        'pe qora',
        '{"id":"builtin-pe-qora","name":"PE qora","active":true,"density_g_cm3":0.92,"variants":[{"micron":30},{"micron":35},{"micron":40},{"micron":45},{"micron":50},{"micron":55},{"micron":60},{"micron":65},{"micron":70},{"micron":75},{"micron":80},{"micron":85},{"micron":90}]}'::jsonb,
        now()
    ),
    (
        'builtin-pe-pr',
        'pe pr',
        '{"id":"builtin-pe-pr","name":"PE PR","active":true,"density_g_cm3":0.92,"variants":[{"micron":30},{"micron":35},{"micron":40},{"micron":45},{"micron":50},{"micron":55},{"micron":60},{"micron":65},{"micron":70},{"micron":75},{"micron":80},{"micron":85},{"micron":90}]}'::jsonb,
        now()
    )
ON CONFLICT DO NOTHING;
