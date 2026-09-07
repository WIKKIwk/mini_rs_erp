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
        let warehouses: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT w.name FROM mini_warehouses w
             JOIN mini_warehouse_assignments a ON lower(a.warehouse_name) = lower(w.name)
             WHERE a.assignment_kind = 'warehouse' AND a.principal_role = 'tayyorlov_masteri'
               AND a.principal_ref = $1 AND NOT w.is_group ORDER BY w.name",
        )
        .bind(owner)
        .fetch_all(&mut *tx)
        .await?;
        let materials: Vec<Value> = sqlx::query_scalar(&format!(
            "SELECT jsonb_build_object('item_code', i.code, 'name', i.name, 'balances',
                COALESCE((SELECT jsonb_agg(b) FROM (
                    SELECT s.warehouse, sum(s.qty)::text AS kg
                    FROM mini_preparation_receipts r
                    JOIN mini_raw_material_stock s ON s.id = r.stock_id
                    WHERE r.owner_ref = $1 AND r.item_code = i.code
                      AND s.warehouse = ANY($2) AND {AVAILABLE_STOCK}
                    GROUP BY s.warehouse
                ) b), '[]'::jsonb))
             FROM mini_preparation_materials m JOIN mini_items i ON i.code = m.item_code
             WHERE m.owner_ref = $1 ORDER BY lower(i.name), i.code"
        ))
        .bind(owner)
        .bind(&warehouses)
        .fetch_all(&mut *tx)
        .await?;
        let orders: Vec<Value> = sqlx::query_scalar(
            "SELECT jsonb_build_object('id', m.id, 'code', m.code, 'title', m.title,
                 'order_kg', round((m.map_json->>'order_kg')::numeric, 6)::text,
                 'saved', EXISTS(SELECT 1 FROM mini_preparation_operations p
                     WHERE p.owner_ref = $1 AND p.order_id = m.id AND p.kind = 'consumption'))
             FROM mini_production_maps m
             WHERE m.lifecycle_status IN ('released', 'in_progress')
               AND jsonb_typeof(m.map_json->'order_kg') = 'number'
               AND (m.map_json->>'order_kg')::numeric > 0
             ORDER BY m.created_at DESC, m.id",
        )
        .bind(owner)
        .fetch_all(&mut *tx)
        .await?;
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
            json!({"warehouses": warehouses, "materials": materials, "orders": orders, "history": history}),
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
        let request = json!({"kind": "material", "input": &input});
        let mut tx = self.begin(&actor.ref_, &input.request_id).await?;
        if let Some(result) = replay(&mut tx, &actor.ref_, &input.request_id, &request).await? {
            return Ok(result);
        }
        let duplicate: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_preparation_materials WHERE owner_ref = $1 AND name_key = lower($2))")
            .bind(&actor.ref_).bind(&name).fetch_one(&mut *tx).await?;
        if duplicate {
            return Err(PreparationError::Conflict("Bunday homashyo nomi mavjud"));
        }
        let id = new_id();
        let code = format!("PREP-{id}");
        sqlx::query(
            "INSERT INTO mini_items (code, name, uom, item_group, payload_json)
             VALUES ($1, $2, 'kg', 'Tayyorlov homashyolari', $3)",
        )
        .bind(&code)
        .bind(&name)
        .bind(json!({"source": "preparation", "owner_ref": actor.ref_}))
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO mini_preparation_materials(item_code, owner_ref, name_key) VALUES ($1,$2,lower($3))")
            .bind(&code).bind(&actor.ref_).bind(&name).execute(&mut *tx).await?;
        let result = json!({"id": id, "kind": "material", "item_code": code, "name": name});
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
        let warehouse = warehouse(&mut tx, &actor.ref_, &input.warehouse).await?;
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
        let warehouse = warehouse(&mut tx, &actor.ref_, &input.warehouse).await?;
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

async fn warehouse(
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
