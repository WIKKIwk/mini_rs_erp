async fn material_name_in_warehouse(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    code: &str,
    warehouse: &str,
) -> Result<String, PreparationError> {
    sqlx::query_scalar(
        "SELECT i.name FROM mini_preparation_materials m
         JOIN mini_items i ON i.code=m.item_code
         WHERE m.owner_ref=$1 AND m.item_code=$2
           AND (
               EXISTS (
                   SELECT 1
                   FROM mini_preparation_material_warehouse_scopes scope
                   JOIN mini_warehouses warehouse ON warehouse.id = scope.warehouse_id
                   WHERE scope.item_code = m.item_code
                     AND scope.active
                     AND lower(warehouse.name) = lower($3)
               )
               OR (
                   NOT EXISTS (
                       SELECT 1
                       FROM mini_preparation_material_warehouse_scopes scope
                       WHERE scope.item_code = m.item_code
                   )
                   AND (
                       m.warehouse_name IS NULL
                       OR lower(m.warehouse_name) = lower($3)
                   )
               )
           )
         FOR SHARE OF i",
    )
    .bind(owner)
    .bind(code)
    .bind(warehouse.trim())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::MaterialNotInWarehouse)
}

async fn assigned_rulon_item_for_receipt(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    item_code: &str,
) -> Result<(String, String), PreparationError> {
    sqlx::query_as(
        "WITH RECURSIVE rulon_groups AS (
            SELECT g.name
            FROM mini_item_groups g
            WHERE lower(btrim(g.name)) IN ('rulon', 'rulon materiallari')
              AND g.is_group
            UNION
            SELECT child.name
            FROM mini_item_groups child
            JOIN rulon_groups parent
              ON lower(btrim(child.parent_item_group)) = lower(btrim(parent.name))
            WHERE child.is_group
        )
        SELECT i.code, i.name
        FROM mini_items i
        WHERE lower(btrim(i.code)) = lower(btrim($2))
          AND EXISTS (
              SELECT 1
              FROM mini_preparation_material_responsibilities r
              WHERE r.principal_role = 'tayyorlov_masteri'
                AND r.principal_ref = $1
                AND (
                    lower(btrim(r.material_name)) = lower(btrim(i.code))
                    OR lower(btrim(r.material_name)) = lower(btrim(i.name))
                    OR lower(btrim(r.material_id)) = lower(btrim(i.code))
                    OR lower(btrim(r.material_id)) = lower(btrim(i.name))
                )
          )
          AND EXISTS (
              SELECT 1
              FROM rulon_groups allowed
              WHERE lower(btrim(allowed.name)) = lower(btrim(i.item_group))
          )
        LIMIT 1",
    )
    .bind(owner.trim())
    .bind(item_code.trim())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::Forbidden)
}

/// Calculate katalogdagi kanonik nom: avval DB override, keyin builtin defaultlar.
/// Katalog list() bilan bir xil mantiq — picker'dagi har qanday id topiladi.
async fn calculate_material_name_by_id(
    pool: &PgPool,
    material_id: &str,
) -> Result<Option<String>, PreparationError> {
    let name: Option<String> = sqlx::query_scalar(
        "SELECT COALESCE(payload_json->>'name', id) FROM mini_calculate_materials WHERE lower(id) = lower($1)",
    )
    .bind(material_id.trim())
    .fetch_optional(pool)
    .await?;
    if let Some(name) = name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        return Ok(Some(name));
    }
    Ok(
        crate::core::calculate_materials::default_calculate_materials()
            .into_iter()
            .find(|m| m.id.trim().eq_ignore_ascii_case(material_id.trim()))
            .map(|m| m.name.trim().to_string())
            .filter(|s| !s.is_empty()),
    )
}

/// Nom bo'yicha katalog id topish (legacy matnli qatlamlarni id'ga ko'tarish uchun).
async fn calculate_material_id_by_name(
    pool: &PgPool,
    name: &str,
) -> Result<Option<(String, String)>, PreparationError> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, COALESCE(payload_json->>'name', id) FROM mini_calculate_materials
         WHERE lower(COALESCE(payload_json->>'name', id)) = lower($1) LIMIT 1",
    )
    .bind(name.trim())
    .fetch_optional(pool)
    .await?;
    if let Some((id, resolved)) = row {
        let resolved = resolved.trim().to_string();
        if !resolved.is_empty() {
            return Ok(Some((id, resolved)));
        }
    }
    Ok(
        crate::core::calculate_materials::default_calculate_materials()
            .into_iter()
            .find(|m| m.name.trim().eq_ignore_ascii_case(name.trim()))
            .map(|m| (m.id.clone(), m.name.clone())),
    )
}

/// Biriktirilgan homashyoning kanonik nomi; biriktirilmagan bo'lsa Forbidden.
/// Tx ichida (upsert) ishlatiladi.
async fn responsibility_material_name(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    material_id: &str,
) -> Result<String, PreparationError> {
    sqlx::query_scalar(
        "SELECT material_name FROM mini_preparation_material_responsibilities
         WHERE principal_role='tayyorlov_masteri' AND principal_ref=$1
           AND lower(material_id)=lower($2)",
    )
    .bind(owner)
    .bind(material_id.trim())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::Forbidden)
}

/// Tx'siz variant (delete uchun).
async fn responsibility_material_name_tx(
    pool: &PgPool,
    owner: &str,
    material_id: &str,
) -> Result<Option<String>, PreparationError> {
    sqlx::query_scalar(
        "SELECT material_name FROM mini_preparation_material_responsibilities
         WHERE principal_role='tayyorlov_masteri' AND principal_ref=$1
           AND lower(material_id)=lower($2)",
    )
    .bind(owner)
    .bind(material_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(PreparationError::from)
}

async fn order_matches_responsibility(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    order_id: &str,
) -> Result<bool, PreparationError> {
    order_matches_material_responsibility(tx, owner, order_id, "").await
}

async fn order_matches_material_responsibility(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    order_id: &str,
    material_id: &str,
) -> Result<bool, PreparationError> {
    let matched: bool = sqlx::query_scalar(
        "SELECT EXISTS (
           SELECT 1 FROM mini_preparation_material_responsibilities r
           WHERE r.principal_role = 'tayyorlov_masteri' AND r.principal_ref = $1
             AND ($3 = '' OR lower(r.material_id) = lower($3))
             AND (
               EXISTS (
                 SELECT 1 FROM mini_order_products p
                 LEFT JOIN LATERAL jsonb_array_elements(COALESCE(p.layers_json, '[]'::jsonb)) l ON true
                 WHERE p.order_id = $2 AND (
                   lower(COALESCE(l->>'material_id','')) = lower(r.material_id)
                   OR lower(COALESCE(l->>'material','')) = lower(r.material_name)
                   OR lower(COALESCE(p.first_layer_material,'')) = lower(r.material_name)
                   OR lower(COALESCE(p.second_layer_material,'')) = lower(r.material_name)
                   OR lower(COALESCE(p.third_layer_material,'')) = lower(r.material_name)
                 )
               )
               OR EXISTS (
                 SELECT 1 FROM mini_quick_order_templates t
                 LEFT JOIN LATERAL jsonb_array_elements(COALESCE(t.payload_json->'layers', '[]'::jsonb)) l ON true
                 WHERE btrim(COALESCE(t.payload_json->>'source_map_id','')) = $2
                   AND NOT EXISTS (
                     SELECT 1 FROM mini_order_products current_product
                     WHERE current_product.order_id = $2
                   )
                   AND (
                     lower(COALESCE(l->>'material_id','')) = lower(r.material_id)
                     OR lower(COALESCE(l->>'material','')) = lower(r.material_name)
                     OR lower(COALESCE(t.payload_json->>'first_layer_material','')) = lower(r.material_name)
                     OR lower(COALESCE(t.payload_json->>'second_layer_material','')) = lower(r.material_name)
                     OR lower(COALESCE(t.payload_json->>'third_layer_material','')) = lower(r.material_name)
                   )
               )
             )
         )",
    )
    .bind(owner)
    .bind(order_id)
    .bind(material_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(matched)
}

#[allow(clippy::too_many_arguments)]
async fn record(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    owner: &str,
    key: &str,
    kind: &str,
    order: Option<&str>,
    request: Value,
    response: &Value,
) -> Result<(), PreparationError> {
    sqlx::query("INSERT INTO mini_preparation_operations(id,owner_ref,request_id,kind,order_id,request_json,response_json) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(id).bind(owner).bind(key).bind(kind).bind(order).bind(request).bind(response)
        .execute(&mut **tx).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn event(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Principal,
    id: &str,
    warehouse: &str,
    barcode: &str,
    code: &str,
    name: &str,
    delta: i64,
    order: Option<&str>,
    after: &str,
) -> Result<(), PreparationError> {
    let consuming = order.is_some();
    let delta = if delta < 0 {
        format!("-{}", decimal_text(-delta))
    } else {
        decimal_text(delta)
    };
    sqlx::query("INSERT INTO mini_raw_material_events
        (event_id,idempotency_key,event_type,warehouse,barcode,item_code,item_name,qty_delta,
         stock_status_before,stock_status_after,order_id,actor_role,actor_ref,actor_display_name,
         owner_role,owner_ref,owner_display_name,source_type,source_id,source_line_ref,correlation_id,payload_json)
        VALUES ($1,$1,$2,$3,$4,$5,$6,$7::text::numeric,$8,$9,$10,'tayyorlov_masteri',$11,$12,
                'tayyorlov_masteri',$11,$12,$13,$14,$4,$14,$15)")
        .bind(format!("prep:{id}:{barcode}"))
        .bind(if consuming { "consumption_posted" } else { "receipt_posted" })
        .bind(warehouse).bind(barcode).bind(code).bind(name).bind(delta)
        .bind(if consuming { Some("available") } else { None }).bind(after).bind(order)
        .bind(&actor.ref_).bind(&actor.display_name)
        .bind(if consuming { "consumption" } else { "system" }).bind(id)
        .bind(json!({"source":"preparation", "operation_id":id}))
        .execute(&mut **tx).await?;
    Ok(())
}
