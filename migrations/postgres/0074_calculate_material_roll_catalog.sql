DO $$
DECLARE
    raw_group TEXT;
    roll_group TEXT;
BEGIN
    SELECT name
    INTO raw_group
    FROM mini_item_groups
    WHERE lower(btrim(name)) IN ('homashyo', 'xomashyo')
    ORDER BY CASE lower(btrim(name)) WHEN 'homashyo' THEN 0 ELSE 1 END, name
    LIMIT 1;

    IF raw_group IS NULL THEN
        raw_group := 'Homashyo';
        INSERT INTO mini_item_groups
            (name, parent_item_group, is_group, payload_json, updated_at)
        VALUES (
            raw_group,
            'All Item Groups',
            true,
            jsonb_build_object(
                'name', raw_group,
                'item_group_name', raw_group,
                'parent_item_group', 'All Item Groups',
                'is_group', true
            ),
            now()
        );
    END IF;

    SELECT name
    INTO roll_group
    FROM mini_item_groups
    WHERE lower(btrim(name)) IN ('rulon', 'rulon materiallari')
    ORDER BY CASE lower(btrim(name)) WHEN 'rulon' THEN 0 ELSE 1 END, name
    LIMIT 1;

    IF roll_group IS NULL THEN
        roll_group := 'Rulon';
        INSERT INTO mini_item_groups
            (name, parent_item_group, is_group, payload_json, updated_at)
        VALUES (
            roll_group,
            raw_group,
            true,
            jsonb_build_object(
                'name', roll_group,
                'item_group_name', roll_group,
                'parent_item_group', raw_group,
                'is_group', true
            ),
            now()
        );
    ELSE
        UPDATE mini_item_groups
        SET parent_item_group = raw_group,
            is_group = true,
            payload_json = payload_json
                || jsonb_build_object(
                    'name', roll_group,
                    'item_group_name', roll_group,
                    'parent_item_group', raw_group,
                    'is_group', true
                ),
            updated_at = now()
        WHERE name = roll_group;
    END IF;
END;
$$;

WITH default_materials(name) AS (
    VALUES
        ('PET'),
        ('OPP'),
        ('BOPP'),
        ('BOPP metal'),
        ('MCP'),
        ('MCPP'),
        ('CPP'),
        ('PE'),
        ('PE oq'),
        ('PE PR'),
        ('JEM')
),
stored_materials(name) AS (
    SELECT btrim(payload_json->>'name')
    FROM mini_calculate_materials
    WHERE id <> 'builtin-pe-qora'
      AND btrim(COALESCE(payload_json->>'name', '')) <> ''
),
material_names(name) AS (
    SELECT name FROM default_materials
    UNION
    SELECT name FROM stored_materials
),
roll_group(name) AS (
    SELECT name
    FROM mini_item_groups
    WHERE lower(btrim(name)) IN ('rulon', 'rulon materiallari')
    ORDER BY CASE lower(btrim(name)) WHEN 'rulon' THEN 0 ELSE 1 END, name
    LIMIT 1
)
UPDATE mini_items AS item
SET item_group = roll_group.name,
    payload_json = jsonb_set(
        item.payload_json,
        '{item_group}',
        to_jsonb(roll_group.name),
        true
    ),
    updated_at = now()
FROM roll_group
WHERE EXISTS (
    SELECT 1
    FROM material_names
    WHERE lower(btrim(material_names.name)) = lower(btrim(item.code))
       OR lower(btrim(material_names.name)) = lower(btrim(item.name))
);

WITH default_materials(name) AS (
    VALUES
        ('PET'),
        ('OPP'),
        ('BOPP'),
        ('BOPP metal'),
        ('MCP'),
        ('MCPP'),
        ('CPP'),
        ('PE'),
        ('PE oq'),
        ('PE PR'),
        ('JEM')
),
stored_materials(name) AS (
    SELECT btrim(payload_json->>'name')
    FROM mini_calculate_materials
    WHERE id <> 'builtin-pe-qora'
      AND btrim(COALESCE(payload_json->>'name', '')) <> ''
),
material_names(name) AS (
    SELECT name FROM default_materials
    UNION
    SELECT name FROM stored_materials
),
roll_group(name) AS (
    SELECT name
    FROM mini_item_groups
    WHERE lower(btrim(name)) IN ('rulon', 'rulon materiallari')
    ORDER BY CASE lower(btrim(name)) WHEN 'rulon' THEN 0 ELSE 1 END, name
    LIMIT 1
)
INSERT INTO mini_items (code, name, uom, item_group, payload_json, updated_at)
SELECT
    material_names.name,
    material_names.name,
    'Kg',
    roll_group.name,
    jsonb_build_object(
        'code', material_names.name,
        'name', material_names.name,
        'uom', 'Kg',
        'item_group', roll_group.name
    ),
    now()
FROM material_names
CROSS JOIN roll_group
WHERE NOT EXISTS (
    SELECT 1
    FROM mini_items AS item
    WHERE lower(btrim(item.code)) = lower(btrim(material_names.name))
       OR lower(btrim(item.name)) = lower(btrim(material_names.name))
);
