impl PostgresPreparationStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn snapshot(&self, owner: &str) -> Result<Value, PreparationError> {
        self.snapshot_inner(owner, None).await
    }

    pub async fn snapshot_with_warehouse_material_scopes(
        &self,
        owner: &str,
        scopes: &PreparationWarehouseMaterialScopes,
    ) -> Result<Value, PreparationError> {
        self.snapshot_inner(owner, Some(scopes)).await
    }

    async fn snapshot_inner(
        &self,
        owner: &str,
        warehouse_scopes: Option<&PreparationWarehouseMaterialScopes>,
    ) -> Result<Value, PreparationError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        // All real warehouses remain visible. Only material_warehouses permit
        // new materials and manual bulk receipts; other destinations need
        // the scale/QR receipt workflow.
        let warehouses: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM mini_warehouses
             WHERE NOT is_group ORDER BY name",
        )
        .fetch_all(&mut *tx)
        .await?;
        let assigned_warehouses: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT w.name FROM mini_warehouses w
             JOIN mini_warehouse_assignments a ON lower(a.warehouse_name) = lower(w.name)
             WHERE a.assignment_kind = 'warehouse' AND a.principal_role = 'tayyorlov_masteri'
               AND a.principal_ref = $1 AND NOT w.is_group ORDER BY w.name",
        )
        .bind(owner)
        .fetch_all(&mut *tx)
        .await?;
        let material_warehouses: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT w.name FROM mini_warehouses w
             JOIN mini_warehouse_assignments mine
               ON lower(mine.warehouse_name) = lower(w.name)
             WHERE mine.assignment_kind = 'warehouse'
               AND mine.principal_role = 'tayyorlov_masteri'
               AND mine.principal_ref = $1
               AND NOT w.is_group
               AND NOT EXISTS (
                   SELECT 1 FROM mini_warehouse_assignments other
                   WHERE other.assignment_kind = 'warehouse'
                     AND lower(other.warehouse_name) = lower(w.name)
                     AND (other.principal_role <> 'tayyorlov_masteri'
                          OR other.principal_ref <> $1)
               )
             ORDER BY w.name",
        )
        .bind(owner)
        .fetch_all(&mut *tx)
        .await?;
        let managed_warehouses: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM mini_warehouses WHERE preparation_owner_ref = $1
             AND name = ANY($2) AND parent_warehouse <> '' ORDER BY name",
        )
        .bind(owner)
        .bind(&material_warehouses)
        .fetch_all(&mut *tx)
        .await?;
        // The warehouse view uses the shared ERP raw-material catalog and its
        // shared stock. Legacy preparation-owned items outside that catalog
        // remain visible so existing preparation lots stay addressable.
        let balance_warehouses = warehouse_scopes
            .map(|scopes| scopes.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_else(|| warehouses.clone());
        let materials: Vec<Value> = sqlx::query_scalar(&format!(
            "WITH RECURSIVE raw_groups AS (
                SELECT g.name
                FROM mini_item_groups g
                WHERE lower(btrim(g.name)) = lower($1) AND g.is_group
                UNION
                SELECT child.name
                FROM mini_item_groups child
                JOIN raw_groups parent
                  ON lower(btrim(child.parent_item_group)) = lower(btrim(parent.name))
                WHERE child.is_group
            ),
            seriyo_groups AS (
                SELECT g.name
                FROM mini_item_groups g
                WHERE lower(btrim(g.name)) = lower($4) AND g.is_group
                UNION
                SELECT child.name
                FROM mini_item_groups child
                JOIN seriyo_groups parent
                  ON lower(btrim(child.parent_item_group)) = lower(btrim(parent.name))
                WHERE child.is_group
            ),
            catalog AS (
                SELECT i.code, i.name, i.item_group, own.warehouse_name,
                       COALESCE((
                           SELECT array_agg(scope_warehouse.name ORDER BY lower(scope_warehouse.name))
                           FROM mini_preparation_material_warehouse_scopes scope
                           JOIN mini_warehouses scope_warehouse
                             ON scope_warehouse.id = scope.warehouse_id
                           WHERE scope.item_code = i.code AND scope.active
                       ), ARRAY[]::text[]) AS bound_warehouses,
                       EXISTS (
                           SELECT 1
                           FROM mini_preparation_material_warehouse_scopes scope
                           WHERE scope.item_code = i.code
                       ) AS has_warehouse_scope,
                       own.item_code IS NOT NULL
                       AND EXISTS (
                           SELECT 1 FROM seriyo_groups allowed
                           WHERE lower(btrim(allowed.name)) = lower(btrim(i.item_group))
                       ) AS can_receive
                FROM mini_items i
                JOIN raw_groups g
                  ON lower(btrim(g.name)) = lower(btrim(i.item_group))
                LEFT JOIN mini_preparation_materials own
                  ON own.item_code = i.code AND own.owner_ref = $2
                UNION
                SELECT i.code, i.name, i.item_group, own.warehouse_name,
                       COALESCE((
                           SELECT array_agg(scope_warehouse.name ORDER BY lower(scope_warehouse.name))
                           FROM mini_preparation_material_warehouse_scopes scope
                           JOIN mini_warehouses scope_warehouse
                             ON scope_warehouse.id = scope.warehouse_id
                           WHERE scope.item_code = i.code AND scope.active
                       ), ARRAY[]::text[]) AS bound_warehouses,
                       EXISTS (
                           SELECT 1
                           FROM mini_preparation_material_warehouse_scopes scope
                           WHERE scope.item_code = i.code
                       ) AS has_warehouse_scope,
                       EXISTS (
                           SELECT 1 FROM seriyo_groups allowed
                           WHERE lower(btrim(allowed.name)) = lower(btrim(i.item_group))
                       ) AS can_receive
                FROM mini_preparation_materials own
                JOIN mini_items i ON i.code = own.item_code
                WHERE own.owner_ref = $2
            )
            SELECT jsonb_build_object(
                'item_code', catalog_item.code,
                'name', catalog_item.name,
                'item_group', catalog_item.item_group,
                'warehouse_name', catalog_item.warehouse_name,
                'bound_warehouses', to_jsonb(catalog_item.bound_warehouses),
                'has_warehouse_scope', catalog_item.has_warehouse_scope,
                'can_receive', catalog_item.can_receive,
                'balances', COALESCE((SELECT jsonb_agg(b ORDER BY lower(b.warehouse), b.warehouse) FROM (
                    SELECT s.warehouse, sum(s.qty)::text AS kg
                    FROM mini_raw_material_stock s
                    WHERE lower(s.item_code) = lower(catalog_item.code)
                      AND s.warehouse = ANY($3)
                      AND (
                          (
                              catalog_item.has_warehouse_scope
                              AND EXISTS (
                                  SELECT 1
                                  FROM unnest(catalog_item.bound_warehouses) AS scoped(warehouse_name)
                                  WHERE lower(scoped.warehouse_name) = lower(s.warehouse)
                              )
                          )
                          OR (
                              NOT catalog_item.has_warehouse_scope
                              AND (
                                  catalog_item.warehouse_name IS NULL
                                  OR lower(catalog_item.warehouse_name) = lower(s.warehouse)
                              )
                          )
                      )
                      AND {AVAILABLE_STOCK}
                    GROUP BY s.warehouse
                ) b), '[]'::jsonb)
            )
            FROM catalog catalog_item
            ORDER BY lower(catalog_item.name), catalog_item.code"
        ))
        .bind(PREPARATION_ITEM_GROUP)
        .bind(owner)
        .bind(&balance_warehouses)
        .bind(PREPARATION_MATERIAL_CHILD_GROUP)
        .fetch_all(&mut *tx)
        .await?;
        let materials: Vec<Value> = materials
            .into_iter()
            .map(|mut material| {
                let item_group = material
                    .get("item_group")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if let Some(scopes) = warehouse_scopes {
                    let can_receive = material
                        .get("can_receive")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let bound_warehouse = material
                        .get("warehouse_name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|warehouse| !warehouse.is_empty());
                    let bound_warehouses = material
                        .get("bound_warehouses")
                        .and_then(Value::as_array)
                        .map(|warehouses| {
                            warehouses
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::trim)
                                .filter(|warehouse| !warehouse.is_empty())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let has_warehouse_scope = material
                        .get("has_warehouse_scope")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let visible_warehouses = scopes
                        .iter()
                        .filter_map(|(warehouse, scope)| {
                            let in_scope = match scope {
                                PreparationWarehouseMaterialScope::OwnSeriyo => can_receive,
                                PreparationWarehouseMaterialScope::AssignedItemGroups(groups) => {
                                    groups
                                        .iter()
                                        .any(|group| group.trim().eq_ignore_ascii_case(&item_group))
                                }
                            };
                            let matches_binding = if has_warehouse_scope {
                                bound_warehouses
                                    .iter()
                                    .any(|bound| warehouse.eq_ignore_ascii_case(bound))
                            } else {
                                bound_warehouse
                                    .map(|bound| warehouse.eq_ignore_ascii_case(bound))
                                    .unwrap_or(true)
                            };
                            (in_scope && matches_binding).then(|| warehouse.clone())
                        })
                        .collect::<Vec<_>>();
                    material["visible_warehouses"] = json!(visible_warehouses);
                }
                if let Some(object) = material.as_object_mut() {
                    object.remove("item_group");
                    object.remove("warehouse_name");
                    object.remove("bound_warehouses");
                    object.remove("has_warehouse_scope");
                }
                material
            })
            .collect();
        // Javobgar homashyolar (calculate-material id, micron'siz).
        // Biriktirilmagan master fail-closed: order list bo'sh.
        let responsibilities: Vec<Value> = sqlx::query_scalar(
            "SELECT jsonb_build_object('material_id', material_id, 'material_name', material_name)
             FROM mini_preparation_material_responsibilities
             WHERE principal_role = 'tayyorlov_masteri' AND principal_ref = $1
             ORDER BY lower(material_name), lower(material_id)",
        )
        .bind(owner)
        .fetch_all(&mut *tx)
        .await?;
        let assigned_ids: Vec<String> = responsibilities
            .iter()
            .filter_map(|v| {
                v.get("material_id")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_lowercase())
            })
            .collect();
        let assigned_names: Vec<String> = responsibilities
            .iter()
            .filter_map(|v| {
                v.get("material_name")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_lowercase())
            })
            .collect();
        let orders: Vec<Value> = if assigned_ids.is_empty() {
            Vec::new()
        } else {
            // Current order products are authoritative. Templates are only a
            // fallback by exact map identity, never by reusable code/number.
            sqlx::query_scalar(
                "SELECT jsonb_build_object('id', m.id, 'code', m.code, 'title', m.title,
                     'order_kg', round((m.map_json->>'order_kg')::numeric, 6)::text,
                     'width_mm', CASE WHEN jsonb_typeof(m.map_json->'width_mm') = 'number'
                        THEN round((m.map_json->>'width_mm')::numeric, 3)::text ELSE NULL END,
                     'saved', EXISTS(SELECT 1 FROM mini_preparation_operations p
                         WHERE p.owner_ref = $1 AND p.order_id = m.id AND p.kind = 'consumption'))
                 FROM mini_production_maps m
                 WHERE m.lifecycle_status IN ('released', 'in_progress')
                   AND jsonb_typeof(m.map_json->'order_kg') = 'number'
                   AND (m.map_json->>'order_kg')::numeric > 0
                   AND (
                     EXISTS (
                       SELECT 1 FROM mini_order_products p
                       LEFT JOIN LATERAL jsonb_array_elements(COALESCE(p.layers_json, '[]'::jsonb)) l ON true
                       WHERE p.order_id = m.id AND (
                         lower(COALESCE(l->>'material_id','')) = ANY($2)
                         OR lower(COALESCE(l->>'material','')) = ANY($3)
                         OR lower(COALESCE(p.first_layer_material,'')) = ANY($3)
                         OR lower(COALESCE(p.second_layer_material,'')) = ANY($3)
                         OR lower(COALESCE(p.third_layer_material,'')) = ANY($3)
                       )
                     )
                     OR EXISTS (
                       SELECT 1 FROM mini_quick_order_templates t
                       LEFT JOIN LATERAL jsonb_array_elements(COALESCE(t.payload_json->'layers', '[]'::jsonb)) l ON true
                       WHERE btrim(COALESCE(t.payload_json->>'source_map_id','')) = m.id
                         AND NOT EXISTS (
                           SELECT 1 FROM mini_order_products current_product
                           WHERE current_product.order_id = m.id
                         )
                         AND (
                           lower(COALESCE(l->>'material_id','')) = ANY($2)
                           OR lower(COALESCE(l->>'material','')) = ANY($3)
                           OR lower(COALESCE(t.payload_json->>'first_layer_material','')) = ANY($3)
                           OR lower(COALESCE(t.payload_json->>'second_layer_material','')) = ANY($3)
                           OR lower(COALESCE(t.payload_json->>'third_layer_material','')) = ANY($3)
                         )
                     )
                   )
                 ORDER BY m.created_at DESC, m.id",
            )
            .bind(owner)
            .bind(&assigned_ids)
            .bind(&assigned_names)
            .fetch_all(&mut *tx)
            .await?
        };
        let history = history_documents::operation_history_documents(&mut tx, owner, None, 100).await?;
        tx.commit().await?;
        Ok(
            json!({"warehouses": warehouses, "assigned_warehouses": assigned_warehouses,
                "material_warehouses": material_warehouses, "managed_warehouses": managed_warehouses,
                "materials": materials,
                "orders": orders, "history": history, "responsibilities": responsibilities}),
        )
    }
}
