use crate::core::auth::models::Principal;
use crate::core::preparation::*;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};

impl From<sqlx::Error> for PreparationError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "preparation storage operation failed");
        PreparationError::StoreFailed
    }
}

// Reuse the shared stock and physical-location authority. A lot can be
// accounted to this warehouse but physically at an apparatus or in transit.
const AVAILABLE_STOCK: &str = "s.status = 'available' AND s.reserved_order_id = ''
    AND btrim(COALESCE(s.payload_json->>'inventory_transfer_id', '')) = ''
    AND NOT EXISTS (SELECT 1 FROM mini_raw_material_assignments a WHERE lower(a.barcode) = lower(s.barcode))
    AND NOT EXISTS (
        SELECT 1 FROM mini_inventory_placements p
        JOIN mini_inventory_locations l ON l.id = p.physical_location_id
        LEFT JOIN mini_warehouses w ON w.id = l.warehouse_id
        WHERE p.asset_kind = 'raw_material' AND lower(p.asset_ref) = lower(s.id)
          AND (l.kind <> 'warehouse' OR w.name IS NULL OR lower(w.name) <> lower(s.warehouse)))";

const PREPARATION_ITEM_GROUP: &str = "Homashyo";
const PREPARATION_MATERIAL_CHILD_GROUP: &str = "seriyo";

#[derive(Clone)]
pub struct PostgresPreparationStore {
    pool: PgPool,
}

impl PostgresPreparationStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn snapshot(&self, owner: &str) -> Result<Value, PreparationError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        // Receipt destination is deliberately broader than the master's own
        // warehouses: an existing preparation material may be received into
        // any real warehouse.  The stricter list below is used only for
        // creating a new material in the selected warehouse.
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
        // The warehouse view uses the shared ERP raw-material catalog and its
        // shared stock. Legacy preparation-owned items outside that catalog
        // remain visible so existing preparation lots stay addressable.
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
            catalog AS (
                SELECT i.code, i.name,
                       EXISTS (
                           SELECT 1 FROM mini_preparation_materials own
                           WHERE own.item_code = i.code AND own.owner_ref = $2
                       ) AS can_receive
                FROM mini_items i
                JOIN raw_groups g
                  ON lower(btrim(g.name)) = lower(btrim(i.item_group))
                UNION
                SELECT i.code, i.name, TRUE
                FROM mini_preparation_materials own
                JOIN mini_items i ON i.code = own.item_code
                WHERE own.owner_ref = $2
            )
            SELECT jsonb_build_object(
                'item_code', catalog_item.code,
                'name', catalog_item.name,
                'can_receive', catalog_item.can_receive,
                'balances', COALESCE((SELECT jsonb_agg(b ORDER BY lower(b.warehouse), b.warehouse) FROM (
                    SELECT s.warehouse, sum(s.qty)::text AS kg
                    FROM mini_raw_material_stock s
                    WHERE lower(s.item_code) = lower(catalog_item.code)
                      AND s.warehouse = ANY($3) AND {AVAILABLE_STOCK}
                    GROUP BY s.warehouse
                ) b), '[]'::jsonb)
            )
            FROM catalog catalog_item
            ORDER BY lower(catalog_item.name), catalog_item.code"
        ))
        .bind(PREPARATION_ITEM_GROUP)
        .bind(owner)
        .bind(&warehouses)
        .fetch_all(&mut *tx)
        .await?;
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
                       WHERE (btrim(COALESCE(t.payload_json->>'source_map_id','')) = m.id
                              OR (btrim(COALESCE(t.payload_json->>'order_number','')) <> ''
                                  AND btrim(COALESCE(t.payload_json->>'order_number','')) = m.order_number)
                              OR (btrim(COALESCE(t.code,'')) <> '' AND btrim(COALESCE(t.code,'')) = m.code))
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
        let history: Vec<Value> = sqlx::query_scalar(
            "SELECT response_json || jsonb_build_object('created_at', created_at)
             FROM mini_preparation_operations WHERE owner_ref = $1 AND kind <> 'material'
             ORDER BY created_at DESC, id DESC LIMIT 100",
        )
        .bind(owner)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(
            json!({"warehouses": warehouses, "assigned_warehouses": assigned_warehouses,
                "material_warehouses": material_warehouses, "materials": materials,
                "orders": orders, "history": history, "responsibilities": responsibilities}),
        )
    }

    pub async fn create_material(
        &self,
        actor: &Principal,
        input: MaterialCreate,
    ) -> Result<Value, PreparationError> {
        let name = input.name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() || name.chars().count() > 160 {
            return Err(PreparationError::Invalid(
                "Homashyo nomi 1–160 ta belgidan iborat bo‘lishi kerak",
            ));
        }
        if input.warehouse.trim().is_empty() {
            return Err(PreparationError::Invalid("Ombor tanlanmagan"));
        }
        let request = json!({"kind": "material", "input": &input});
        let mut tx = self.begin(&actor.ref_, &input.request_id).await?;
        if let Some(result) = replay(&mut tx, &actor.ref_, &input.request_id, &request).await? {
            return Ok(result);
        }
        let warehouse = exclusive_warehouse(&mut tx, &actor.ref_, &input.warehouse).await?;
        let duplicate: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_preparation_materials WHERE owner_ref = $1 AND name_key = lower($2))")
            .bind(&actor.ref_).bind(&name).fetch_one(&mut *tx).await?;
        if duplicate {
            return Err(PreparationError::Conflict("Bunday homashyo nomi mavjud"));
        }
        let item_group = preparation_material_group(&mut tx).await?;
        let id = new_id();
        let code = format!("PREP-{id}");
        sqlx::query(
            "INSERT INTO mini_items (code, name, uom, item_group, payload_json)
             VALUES ($1, $2, 'kg', $3, $4)",
        )
        .bind(&code)
        .bind(&name)
        .bind(&item_group)
        .bind(json!({"source": "preparation", "owner_ref": actor.ref_,
            "item_group": item_group}))
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO mini_preparation_materials(item_code, owner_ref, name_key) VALUES ($1,$2,lower($3))")
            .bind(&code).bind(&actor.ref_).bind(&name).execute(&mut *tx).await?;
        let result = json!({"id": id, "kind": "material", "warehouse": warehouse,
            "item_group": item_group, "item_code": code, "name": name});
        record(
            &mut tx,
            &id,
            &actor.ref_,
            &input.request_id,
            "material",
            None,
            request,
            &result,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn receive(
        &self,
        actor: &Principal,
        input: ReceiptCreate,
    ) -> Result<Value, PreparationError> {
        let kg = decimal(&input.kg)?;
        let request = json!({"kind": "receipt", "input": &input});
        let mut tx = self.begin(&actor.ref_, &input.request_id).await?;
        if let Some(result) = replay(&mut tx, &actor.ref_, &input.request_id, &request).await? {
            return Ok(result);
        }
        let warehouse = receipt_warehouse(&mut tx, &input.warehouse).await?;
        let name = material_name(&mut tx, &actor.ref_, &input.item_code).await?;
        let id = new_id();
        let stock_id = format!("raw:prep:{id}");
        let barcode = format!("PREP-{id}");
        sqlx::query(
            "INSERT INTO mini_raw_material_stock
            (id, warehouse, item_code, item_name, barcode, qty, source_receipt_id, payload_json)
            VALUES ($1,$2,$3,$4,$5,$6::text::numeric,$7,$8)",
        )
        .bind(&stock_id)
        .bind(&warehouse)
        .bind(&input.item_code)
        .bind(&name)
        .bind(&barcode)
        .bind(decimal_text(kg))
        .bind(&id)
        .bind(
            json!({"source":"preparation", "owner_ref":actor.ref_, "initial_kg":decimal_text(kg)}),
        )
        .execute(&mut *tx)
        .await?;
        let result = json!({"id":id, "kind":"receipt", "warehouse":warehouse, "item_code":input.item_code,
            "name":name, "kg":decimal_text(kg), "barcode":barcode});
        record(
            &mut tx,
            &id,
            &actor.ref_,
            &input.request_id,
            "receipt",
            None,
            request,
            &result,
        )
        .await?;
        sqlx::query("INSERT INTO mini_preparation_receipts(id,stock_id,item_code,owner_ref,initial_kg) VALUES ($1,$2,$3,$4,$5::text::numeric)")
            .bind(&id).bind(&stock_id).bind(&input.item_code).bind(&actor.ref_).bind(decimal_text(kg))
            .execute(&mut *tx).await?;
        event(
            &mut tx,
            actor,
            &id,
            &warehouse,
            &barcode,
            &input.item_code,
            &name,
            kg,
            None,
            "available",
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn consume(
        &self,
        actor: &Principal,
        input: ConsumptionCreate,
    ) -> Result<Value, PreparationError> {
        let expected_kg = decimal(&input.expected_order_kg)?;
        let quantities = input.quantities(expected_kg)?;
        let request = json!({"kind": "consumption", "input": &input});
        let mut tx = self.begin(&actor.ref_, &input.request_id).await?;
        if let Some(result) = replay(&mut tx, &actor.ref_, &input.request_id, &request).await? {
            return Ok(result);
        }
        let warehouse = assigned_warehouse(&mut tx, &actor.ref_, &input.warehouse).await?;
        let order = sqlx::query(
            "SELECT code, title, lifecycle_status,
                round((map_json->>'order_kg')::numeric, 6)::text AS kg
             FROM mini_production_maps WHERE id = $1 FOR UPDATE",
        )
        .bind(&input.order_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(PreparationError::Invalid("Order topilmadi"))?;
        let status: String = order.try_get("lifecycle_status")?;
        if !matches!(status.as_str(), "released" | "in_progress") {
            return Err(PreparationError::Conflict(
                "Bu orderga hozir sarf yozib bo‘lmaydi",
            ));
        }
        let actual: Option<String> = order.try_get("kg")?;
        let actual = actual.ok_or(PreparationError::Invalid("Order KG miqdori kiritilmagan"))?;
        if decimal(&actual)? != expected_kg {
            return Err(PreparationError::Conflict(
                "Order KG o‘zgargan. Yangilab, qayta tekshiring",
            ));
        }
        let saved: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_preparation_operations WHERE owner_ref=$1 AND order_id=$2 AND kind='consumption')")
            .bind(&actor.ref_).bind(&input.order_id).fetch_one(&mut *tx).await?;
        if saved {
            return Err(PreparationError::Conflict(
                "Bu order uchun tayyorlov sarfi avval saqlangan",
            ));
        }
        // Scope: order'dagi calculate-qatlamlardan kamida bittasi shu master'ga
        // biriktirilgan bo'lishi shart. UI filtrni chetlab o'tishdan himoya.
        if !order_matches_responsibility(&mut tx, &actor.ref_, &input.order_id).await? {
            return Err(PreparationError::Forbidden);
        }
        let id = new_id();
        let mut lines = Vec::new();
        let mut allocations = Vec::new();
        for (code, percent, required) in quantities {
            let name = material_name(&mut tx, &actor.ref_, &code).await?;
            let lots = sqlx::query(&format!(
                "SELECT r.id AS receipt_id, s.id, s.barcode, s.qty::text AS kg
                FROM mini_preparation_receipts r JOIN mini_raw_material_stock s ON s.id = r.stock_id
                WHERE r.owner_ref=$1 AND r.item_code=$2 AND s.warehouse=$3
                  AND {AVAILABLE_STOCK}
                ORDER BY r.created_at, r.id FOR UPDATE OF s"
            ))
            .bind(&actor.ref_)
            .bind(&code)
            .bind(&warehouse)
            .fetch_all(&mut *tx)
            .await?;
            let mut needed = required;
            for lot in lots {
                if needed == 0 {
                    break;
                }
                let available = decimal(&lot.try_get::<String, _>("kg")?)?;
                let used = available.min(needed);
                let remaining = available - used;
                let stock_id: String = lot.try_get("id")?;
                let barcode: String = lot.try_get("barcode")?;
                let receipt_id: String = lot.try_get("receipt_id")?;
                // READ COMMITTED takes a fresh snapshot here after any lock
                // wait. Assigning a barcode can insert a link without changing
                // the stock row; the pre-lock NOT EXISTS alone is insufficient.
                let eligible: bool = sqlx::query_scalar(&format!("SELECT EXISTS(SELECT 1 FROM mini_raw_material_stock s WHERE s.id=$1 AND {AVAILABLE_STOCK})"))
                    .bind(&stock_id).fetch_one(&mut *tx).await?;
                if !eligible {
                    continue;
                }
                let after = if remaining == 0 {
                    "consumed"
                } else {
                    "available"
                };
                // Existing ERP whole-asset convention retains qty on consumed
                // rows. Effective balance is zero by status; partial rows hold
                // the remainder. Original receipt quantity stays immutable.
                sqlx::query("UPDATE mini_raw_material_stock SET qty=$2::text::numeric, status=$3, updated_at=now() WHERE id=$1")
                    .bind(&stock_id).bind(decimal_text(if remaining == 0 { available } else { remaining }))
                    .bind(after).execute(&mut *tx).await?;
                event(
                    &mut tx,
                    actor,
                    &id,
                    &warehouse,
                    &barcode,
                    &code,
                    &name,
                    -used,
                    Some(&input.order_id),
                    after,
                )
                .await?;
                allocations.push((receipt_id, used));
                needed -= used;
            }
            if needed != 0 {
                return Err(PreparationError::Insufficient);
            }
            lines.push(json!({"item_code":code,"name":name,"percent":decimal_text(percent),"kg":decimal_text(required)}));
        }
        let result = json!({"id":id,"kind":"consumption","warehouse":warehouse,
            "order_id":input.order_id,"order_code":order.try_get::<String,_>("code")?,
            "order_title":order.try_get::<String,_>("title")?,"order_kg":decimal_text(expected_kg),"lines":lines});
        record(
            &mut tx,
            &id,
            &actor.ref_,
            &input.request_id,
            "consumption",
            Some(&input.order_id),
            request,
            &result,
        )
        .await?;
        for (receipt, kg) in allocations {
            sqlx::query("INSERT INTO mini_preparation_allocations(operation_id,receipt_id,kg) VALUES ($1,$2,$3::text::numeric)")
                .bind(&id).bind(receipt).bind(decimal_text(kg)).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(result)
    }

    pub async fn upsert_formula(
        &self,
        actor: &Principal,
        input: FormulaUpsert,
    ) -> Result<Value, PreparationError> {
        let product_code = input.product_key()?;
        let name = input.formula_name()?;
        let material_id = input.material_key()?;
        let normalized = input.normalized_lines()?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '5s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("preparation:formula:{}", actor.ref_))
            .execute(&mut *tx)
            .await?;
        // Scope: formula faqat o'ziga biriktirilgan homashyoga yoziladi.
        let scope_material_name =
            responsibility_material_name(&mut tx, &actor.ref_, &material_id).await?;
        // Resolve names in code order (deadlock-safe), then sort lines
        // alphabetically by material name for stable display.
        let mut lines: Vec<(String, String, i64)> = Vec::new();
        for (code, percent) in normalized {
            let material = material_name(&mut tx, &actor.ref_, &code).await?;
            lines.push((code, material, percent));
        }
        lines.sort_by(|a, b| {
            a.1.to_lowercase()
                .cmp(&b.1.to_lowercase())
                .then_with(|| a.0.cmp(&b.0))
        });
        let payload: Vec<Value> = lines
            .iter()
            .map(|(code, material, percent)| {
                json!({"item_code": code, "name": material, "percent": decimal_text(*percent)})
            })
            .collect();
        sqlx::query(
            "INSERT INTO mini_preparation_formulas(owner_ref, product_code, material_id, material_name, name, lines, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,now())
             ON CONFLICT (owner_ref, product_code, material_id, name)
             DO UPDATE SET material_name = EXCLUDED.material_name, lines = EXCLUDED.lines, updated_at = now()",
        )
        .bind(&actor.ref_)
        .bind(&product_code)
        .bind(&material_id)
        .bind(&scope_material_name)
        .bind(&name)
        .bind(json!(payload))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(
            json!({"product_code": product_code, "material_id": material_id,
            "material_name": scope_material_name, "name": name, "lines": payload}),
        )
    }

    pub async fn list_formulas(
        &self,
        owner: &str,
        product_code: &str,
        material_id: &str,
    ) -> Result<Value, PreparationError> {
        let code = product_code.trim();
        if code.is_empty() || code.chars().count() > 160 {
            return Err(PreparationError::Invalid("Mahsulot kodi noto‘g‘ri"));
        }
        let material = material_id.trim();
        if material.is_empty() || material.chars().count() > 128 {
            return Err(PreparationError::Invalid("Homashyo tanlanmadi"));
        }
        let formulas_value: Value = sqlx::query_scalar(
            "SELECT COALESCE(jsonb_agg(f ORDER BY lower(f->>'name'), f->>'name') FILTER (WHERE f IS NOT NULL), '[]'::jsonb)
             FROM (SELECT jsonb_build_object('name', name, 'lines', lines) AS f
                   FROM mini_preparation_formulas
                   WHERE owner_ref=$1 AND product_code=$2 AND lower(material_id)=lower($3)) s",
        )
        .bind(owner)
        .bind(code)
        .bind(material)
        .fetch_one(&self.pool)
        .await
        .map_err(PreparationError::from)?;
        let formulas = formulas_value.as_array().cloned().unwrap_or_default();
        // Biriktirilmagan homashyo so'rovi bo'sh qaytadi (fail-closed).
        let assigned_name: Option<String> = sqlx::query_scalar(
            "SELECT material_name FROM mini_preparation_material_responsibilities
             WHERE principal_role='tayyorlov_masteri' AND principal_ref=$1
               AND lower(material_id)=lower($2)",
        )
        .bind(owner)
        .bind(material)
        .fetch_optional(&self.pool)
        .await?;
        let Some(assigned_name) = assigned_name else {
            return Ok(json!({"product_code": code, "material_id": material,
                "material_name": "", "formulas": []}));
        };
        Ok(json!({"product_code": code, "material_id": material,
            "material_name": assigned_name, "formulas": formulas}))
    }

    pub async fn delete_formula(
        &self,
        owner: &str,
        product_code: &str,
        name: &str,
        material_id: &str,
    ) -> Result<Value, PreparationError> {
        let code = product_code.trim();
        let formula = name.trim();
        let material = material_id.trim();
        if code.is_empty() || code.chars().count() > 160 {
            return Err(PreparationError::Invalid("Mahsulot kodi noto‘g‘ri"));
        }
        if formula.is_empty() || formula.chars().count() > 80 {
            return Err(PreparationError::Invalid(
                "Formula nomi 1–80 ta belgidan iborat bo‘lishi kerak",
            ));
        }
        if material.is_empty() || material.chars().count() > 128 {
            return Err(PreparationError::Invalid("Homashyo tanlanmadi"));
        }
        // Scope: faqat o'ziga biriktirilgan homashyo formulasi o'chadi.
        responsibility_material_name_tx(&self.pool, owner, material)
            .await?
            .ok_or(PreparationError::Forbidden)?;
        let deleted: bool = sqlx::query_scalar(
            "DELETE FROM mini_preparation_formulas
             WHERE owner_ref=$1 AND product_code=$2 AND name=$3
               AND lower(material_id)=lower($4)
             RETURNING TRUE",
        )
        .bind(owner)
        .bind(code)
        .bind(formula)
        .bind(material)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(false);
        if !deleted {
            return Err(PreparationError::Invalid("Formula topilmadi"));
        }
        Ok(json!({"product_code": code, "material_id": material,
            "name": formula, "deleted": true}))
    }

    /// Order qatlamlaridagi homashyolar (calculate-material oilalari).
    /// Formula tanlash picker'i uchun: master o'z biriktirilganlari bilan
    /// kesishmasini ko'rsatadi.
    pub async fn order_materials(&self, order_id: &str) -> Result<Value, PreparationError> {
        let id = order_id.trim();
        if id.is_empty() {
            return Err(PreparationError::Invalid("Order topilmadi"));
        }
        let pairs: Vec<(String, String)> = sqlx::query_as(
            "SELECT lower(COALESCE(l->>'material_id','')) AS mid,
                    COALESCE(l->>'material','') AS mname
             FROM mini_order_products p,
                  jsonb_array_elements(COALESCE(p.layers_json, '[]'::jsonb)) l
             WHERE p.order_id = $1
             UNION
             SELECT lower(COALESCE(l->>'material_id','')),
                    COALESCE(l->>'material','')
             FROM mini_quick_order_templates t,
                  jsonb_array_elements(COALESCE(t.payload_json->'layers', '[]'::jsonb)) l
             WHERE btrim(COALESCE(t.payload_json->>'source_map_id','')) = $1
             UNION
             SELECT '', COALESCE(p.first_layer_material,'') FROM mini_order_products p
             WHERE p.order_id = $1 AND btrim(COALESCE(p.first_layer_material,'')) <> ''
             UNION
             SELECT '', COALESCE(p.second_layer_material,'') FROM mini_order_products p
             WHERE p.order_id = $1 AND btrim(COALESCE(p.second_layer_material,'')) <> ''
             UNION
             SELECT '', COALESCE(p.third_layer_material,'') FROM mini_order_products p
             WHERE p.order_id = $1 AND btrim(COALESCE(p.third_layer_material,'')) <> ''
             UNION
             SELECT '', COALESCE(t.payload_json->>'first_layer_material','')
             FROM mini_quick_order_templates t
             WHERE btrim(COALESCE(t.payload_json->>'source_map_id','')) = $1
               AND btrim(COALESCE(t.payload_json->>'first_layer_material','')) <> ''
             UNION
             SELECT '', COALESCE(t.payload_json->>'second_layer_material','')
             FROM mini_quick_order_templates t
             WHERE btrim(COALESCE(t.payload_json->>'source_map_id','')) = $1
               AND btrim(COALESCE(t.payload_json->>'second_layer_material','')) <> ''
             UNION
             SELECT '', COALESCE(t.payload_json->>'third_layer_material','')
             FROM mini_quick_order_templates t
             WHERE btrim(COALESCE(t.payload_json->>'source_map_id','')) = $1
               AND btrim(COALESCE(t.payload_json->>'third_layer_material','')) <> ''",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        let mut out: Vec<Value> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for (mid, mname) in pairs {
            let resolved = if mid.trim().is_empty() {
                if mname.trim().is_empty() {
                    continue;
                }
                calculate_material_id_by_name(&self.pool, &mname).await?
            } else {
                let name = calculate_material_name_by_id(&self.pool, &mid)
                    .await?
                    .unwrap_or_else(|| mname.trim().to_string());
                if name.is_empty() {
                    continue;
                }
                Some((mid.trim().to_string(), name))
            };
            if let Some((resolved_id, resolved_name)) = resolved
                && seen.insert(resolved_id.to_lowercase())
            {
                out.push(json!({"material_id": resolved_id,
                    "material_name": resolved_name}));
            }
        }
        out.sort_by(|a, b| {
            a["material_name"]
                .as_str()
                .unwrap_or_default()
                .to_lowercase()
                .cmp(
                    &b["material_name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_lowercase(),
                )
        });
        Ok(json!({"order_id": id, "materials": out}))
    }

    /// Ownerga biriktirilgan omborning kanonik nomi (bola ombor ochishda
    /// ota-ombor tekshiruvi uchun). Biriktirilmagan bo'lsa Forbidden.
    pub async fn owned_warehouse_name(
        &self,
        owner: &str,
        name: &str,
    ) -> Result<String, PreparationError> {
        let mut tx = self.pool.begin().await?;
        let canonical = assigned_warehouse(&mut tx, owner, name).await?;
        tx.rollback().await?;
        Ok(canonical)
    }

    pub async fn warehouse_name_exists(&self, name: &str) -> Result<bool, PreparationError> {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_warehouses WHERE lower(name) = lower($1))")
            .bind(name.trim())
            .fetch_one(&self.pool)
            .await
            .map_err(PreparationError::from)
    }

    pub async fn list_responsibilities(&self, owner: &str) -> Result<Value, PreparationError> {
        let rows: Vec<Value> = sqlx::query_scalar(
            "SELECT jsonb_build_object('material_id', material_id, 'material_name', material_name)
             FROM mini_preparation_material_responsibilities
             WHERE principal_role = 'tayyorlov_masteri' AND principal_ref = $1
             ORDER BY lower(material_name), lower(material_id)",
        )
        .bind(owner.trim())
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({"principal_ref": owner.trim(), "materials": rows}))
    }

    pub async fn assign_responsibility(
        &self,
        input: MaterialResponsibilityAssign,
    ) -> Result<Value, PreparationError> {
        let principal_ref = input.principal_key()?;
        let material_id = input.material_key()?;
        // Faqat mavjud tayyorlov_masteri system user'ga biriktirish.
        let is_master: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mini_system_users WHERE id = $1 AND role = 'tayyorlov_masteri')",
        )
        .bind(&principal_ref)
        .fetch_one(&self.pool)
        .await?;
        if !is_master {
            return Err(PreparationError::Invalid("Tayyorlov masteri topilmadi"));
        }
        // material_id calculate katalogidan bo'lishi shart (micron'siz, oila).
        // Katalog list() dagi kabi: avval DB override, keyin builtin defaultlar.
        let material_name = calculate_material_name_by_id(&self.pool, &material_id)
            .await?
            .ok_or(PreparationError::Invalid("Homashyo katalogda topilmadi"))?;
        sqlx::query(
            "INSERT INTO mini_preparation_material_responsibilities
                 (principal_role, principal_ref, material_id, material_name)
             VALUES ('tayyorlov_masteri', $1, $2, $3)
             ON CONFLICT (principal_role, principal_ref, material_id)
             DO UPDATE SET material_name = EXCLUDED.material_name",
        )
        .bind(&principal_ref)
        .bind(material_id.trim())
        .bind(&material_name)
        .execute(&self.pool)
        .await?;
        Ok(
            json!({"principal_ref": principal_ref, "material_id": material_id.trim(), "material_name": material_name}),
        )
    }

    pub async fn unassign_responsibility(        &self,
        input: MaterialResponsibilityDelete,
    ) -> Result<Value, PreparationError> {
        let principal_ref = input.principal_key()?;
        let material_id = input.material_key()?;
        let deleted: bool = sqlx::query_scalar(
            "DELETE FROM mini_preparation_material_responsibilities
             WHERE principal_role = 'tayyorlov_masteri' AND principal_ref = $1
               AND lower(material_id) = lower($2) RETURNING TRUE",
        )
        .bind(&principal_ref)
        .bind(&material_id)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(false);
        if !deleted {
            return Err(PreparationError::Invalid("Biriktirish topilmadi"));
        }
        Ok(json!({"principal_ref": principal_ref, "material_id": material_id, "deleted": true}))
    }

    /// Master'ga biriktirilgan homashyo oilalari (kichik harfda id + nom).
    /// GScale katalog filtri uchun.
    pub async fn assigned_material_names(
        &self,
        owner: &str,
    ) -> Result<Vec<(String, String)>, PreparationError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT lower(material_id), lower(material_name)
             FROM mini_preparation_material_responsibilities
             WHERE principal_role = 'tayyorlov_masteri' AND principal_ref = $1",
        )
        .bind(owner.trim())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Order shu master'ning biriktirilgan homashyolaridan birini
    /// o'z ichiga oladimi (raw-material ulash scope tekshiruvi uchun).
    pub async fn order_in_scope(        &self,
        owner: &str,
        order_id: &str,
    ) -> Result<bool, PreparationError> {
        let mut tx = self.pool.begin().await?;
        let matched = order_matches_responsibility(&mut tx, owner, order_id).await?;
        tx.rollback().await?;
        Ok(matched)
    }

    async fn begin(
        &self,
        owner: &str,
        request_id: &str,
    ) -> Result<Transaction<'_, Postgres>, PreparationError> {
        if !(8..=128).contains(&request_id.len())
            || !request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_:".contains(&c))
        {
            return Err(PreparationError::Invalid("request_id noto‘g‘ri"));
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '5s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("preparation:{owner}"))
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }
}

fn new_id() -> String {
    data_encoding::HEXLOWER.encode(&rand::random::<[u8; 16]>())
}

async fn replay(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    key: &str,
    request: &Value,
) -> Result<Option<Value>, PreparationError> {
    let row = sqlx::query("SELECT request_json, response_json FROM mini_preparation_operations WHERE owner_ref=$1 AND request_id=$2")
        .bind(owner).bind(key).fetch_optional(&mut **tx).await?;
    match row {
        Some(row) if row.try_get::<Value, _>("request_json")? != *request => Err(
            PreparationError::Conflict("request_id boshqa operatsiya uchun ishlatilgan"),
        ),
        Some(row) => Ok(Some(row.try_get("response_json")?)),
        None => Ok(None),
    }
}

async fn receipt_warehouse(
    tx: &mut Transaction<'_, Postgres>,
    name: &str,
) -> Result<String, PreparationError> {
    sqlx::query_scalar(
        "SELECT w.name FROM mini_warehouses w
         WHERE lower(w.name) = lower($1) AND NOT w.is_group FOR SHARE OF w",
    )
    .bind(name.trim())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::Forbidden)
}

async fn assigned_warehouse(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    name: &str,
) -> Result<String, PreparationError> {
    sqlx::query_scalar(
        "SELECT w.name FROM mini_warehouses w JOIN mini_warehouse_assignments a
            ON lower(a.warehouse_name)=lower(w.name)
        WHERE a.assignment_kind='warehouse' AND a.principal_role='tayyorlov_masteri'
          AND a.principal_ref=$1 AND w.name=$2 AND NOT w.is_group FOR SHARE OF w, a",
    )
    .bind(owner)
    .bind(name.trim())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::Forbidden)
}

async fn exclusive_warehouse(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    name: &str,
) -> Result<String, PreparationError> {
    sqlx::query_scalar(
        "SELECT w.name FROM mini_warehouses w
         JOIN mini_warehouse_assignments mine
           ON lower(mine.warehouse_name) = lower(w.name)
         WHERE mine.assignment_kind = 'warehouse'
           AND mine.principal_role = 'tayyorlov_masteri'
           AND mine.principal_ref = $1
           AND lower(w.name) = lower($2)
           AND NOT w.is_group
           AND NOT EXISTS (
               SELECT 1 FROM mini_warehouse_assignments other
               WHERE other.assignment_kind = 'warehouse'
                 AND lower(other.warehouse_name) = lower(w.name)
                 AND (other.principal_role <> 'tayyorlov_masteri'
                      OR other.principal_ref <> $1)
           )
         FOR SHARE OF w, mine",
    )
    .bind(owner)
    .bind(name.trim())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::WarehouseNotExclusive)
}

async fn preparation_material_group(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<String, PreparationError> {
    // The group is shared by all Tayyorlov masters. Serialize the ensure path
    // so two first-time material creations cannot race on the same group.
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
             hashtextextended('preparation:seriyo-material-group', 0)
         )",
    )
    .execute(&mut **tx)
    .await?;

    if let Some((name, parent, is_group)) = sqlx::query_as::<_, (String, String, bool)>(
        "SELECT name, COALESCE(parent_item_group, ''), is_group
         FROM mini_item_groups
         WHERE lower(name) = lower($1)
         ORDER BY (name = $1) DESC, name
         LIMIT 1
         FOR UPDATE",
    )
    .bind(PREPARATION_MATERIAL_CHILD_GROUP)
    .fetch_optional(&mut **tx)
    .await?
    {
        if !is_group || !parent.eq_ignore_ascii_case(PREPARATION_ITEM_GROUP) {
            return Err(PreparationError::Conflict(
                "Seriyo guruhi Homashyo guruhi ostida emas",
            ));
        }
        return Ok(name);
    }

    let parent = sqlx::query_scalar::<_, String>(
        "SELECT name FROM mini_item_groups
         WHERE lower(name) = lower($1) AND is_group
         ORDER BY (name = $1) DESC, name
         LIMIT 1
         FOR SHARE",
    )
    .bind(PREPARATION_ITEM_GROUP)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::StoreFailed)?;
    let name = sqlx::query_scalar::<_, String>(
        "INSERT INTO mini_item_groups
             (name, parent_item_group, is_group, payload_json, updated_at)
         VALUES ($1, $2, TRUE,
             jsonb_build_object(
                 'name', $1,
                 'item_group_name', $1,
                 'parent_item_group', $2,
                 'is_group', TRUE
             ), now())
         RETURNING name",
    )
    .bind(PREPARATION_MATERIAL_CHILD_GROUP)
    .bind(parent)
    .fetch_one(&mut **tx)
    .await?;
    Ok(name)
}

async fn material_name(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    code: &str,
) -> Result<String, PreparationError> {
    sqlx::query_scalar(
        "SELECT i.name FROM mini_preparation_materials m JOIN mini_items i ON i.code=m.item_code
        WHERE m.owner_ref=$1 AND m.item_code=$2 FOR SHARE OF i",
    )
    .bind(owner)
    .bind(code)
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
    let matched: bool = sqlx::query_scalar(
        "SELECT EXISTS (
           SELECT 1 FROM mini_preparation_material_responsibilities r
           WHERE r.principal_role = 'tayyorlov_masteri' AND r.principal_ref = $1
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
                 WHERE (btrim(COALESCE(t.payload_json->>'source_map_id','')) = $2
                        OR EXISTS (SELECT 1 FROM mini_production_maps m WHERE m.id = $2
                                    AND ((btrim(COALESCE(t.payload_json->>'order_number','')) <> ''
                                          AND btrim(COALESCE(t.payload_json->>'order_number','')) = m.order_number)
                                         OR (btrim(COALESCE(t.code,'')) <> '' AND btrim(COALESCE(t.code,'')) = m.code))))
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
