CREATE INDEX IF NOT EXISTS idx_mini_apparatus_group_id
    ON mini_apparatus(group_id);

CREATE INDEX IF NOT EXISTS idx_mini_customer_items_item_code
    ON mini_customer_items(item_code);

CREATE INDEX IF NOT EXISTS idx_mini_item_groups_parent_exact
    ON mini_item_groups(parent_item_group);

CREATE INDEX IF NOT EXISTS idx_mini_items_item_group_exact
    ON mini_items(item_group);

CREATE INDEX IF NOT EXISTS idx_mini_order_products_order_id
    ON mini_order_products(order_id);

UPDATE mini_production_maps maps
SET order_id = orders.id,
    updated_at = now()
FROM mini_orders orders
WHERE maps.order_id IS NULL
  AND maps.id = orders.id;
