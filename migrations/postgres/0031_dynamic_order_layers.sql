ALTER TABLE mini_order_products
    ADD COLUMN IF NOT EXISTS layers_json JSONB NOT NULL DEFAULT '[]'::jsonb;

UPDATE mini_order_products AS product
SET layers_json = (
    SELECT COALESCE(
        jsonb_agg(
            jsonb_build_object('material', layer.material, 'micron', layer.micron)
            ORDER BY layer.position
        ),
        '[]'::jsonb
    )
    FROM (
        VALUES
            (1, product.first_layer_material, product.first_layer_micron),
            (2, product.second_layer_material, product.second_layer_micron),
            (3, product.third_layer_material, product.third_layer_micron)
    ) AS layer(position, material, micron)
    WHERE btrim(layer.material) <> '' OR btrim(layer.micron) <> ''
)
WHERE product.layers_json = '[]'::jsonb;

DO $$
BEGIN
    ALTER TABLE mini_order_products
        ADD CONSTRAINT mini_order_products_layers_json_array
        CHECK (jsonb_typeof(layers_json) = 'array');
EXCEPTION
    WHEN duplicate_object THEN NULL;
END
$$;
