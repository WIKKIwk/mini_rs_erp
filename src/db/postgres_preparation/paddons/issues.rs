use super::*;
use crate::core::inventory_movements::{
    InventoryActor, InventoryAssetKind, InventoryAssetSelector, InventoryMovementError,
    InventoryRelocationBatchCreate, InventoryTransferCreate, InventoryTransferStatus,
};
use crate::db::postgres_inventory_movements::PostgresInventoryMovementStore;

#[derive(Debug, Deserialize)]
pub struct PaddonIdentity {
    pub code: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
enum Destination {
    Warehouse(String),
    State(String),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaddonIssueCreate {
    request_id: String,
    code: String,
    line_no: i32,
    weights: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    open_unit_no: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_open_kg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remainder_kg: Option<String>,
    #[serde(default)]
    destination: Option<Destination>,
}

impl PaddonIssueCreate {
    pub fn moves_stock(&self) -> bool {
        self.destination.is_some()
    }

    fn quantities(&self) -> Result<Vec<i64>, PreparationError> {
        if !(8..=128).contains(&self.request_id.len())
            || !self
                .request_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
            || self.line_no <= 0
            || !(1..=200).contains(&self.weights.len())
        {
            return Err(PreparationError::Invalid(
                "Paddondan olinadigan birliklar noto‘g‘ri",
            ));
        }
        if let Some(Destination::Warehouse(id) | Destination::State(id)) = &self.destination {
            if id.trim().is_empty() || id.len() > 256 {
                return Err(PreparationError::Invalid("Manzilni tanlang"));
            }
        }
        if self.open_unit_no.is_some() != self.expected_open_kg.is_some()
            || self.open_unit_no.is_some_and(|n| n <= 0)
            || ((self.open_unit_no.is_some() || self.remainder_kg.is_some())
                && self.weights.len() != 1)
        {
            return Err(PreparationError::Invalid(
                "Qisman chiqim uchun bitta birlikni tanlang",
            ));
        }
        self.weights.iter().map(|kg| decimal(kg)).collect()
    }
}

#[derive(sqlx::FromRow)]
struct IssueUnit {
    unit_no: i32,
    stock_id: Option<String>,
    prepared_roll: bool,
    #[sqlx(flatten)]
    material: PaddonMaterial,
}

impl PostgresPreparationStore {
    pub async fn raw_material_paddon_issues(
        &self,
        actor: &Principal,
        raw: &str,
    ) -> Result<Vec<Value>, PreparationError> {
        let code = paddon_code(raw)?;
        Ok(sqlx::query_scalar(
            "SELECT i.result
             FROM mini_raw_material_paddon_issue_history i
             JOIN mini_raw_material_paddons p ON p.id = i.paddon_id
             JOIN mini_warehouses w ON w.id = p.warehouse_id
             WHERE p.code = $1 AND p.owner_role = $2 AND p.owner_ref = $3 AND NOT w.is_group
               AND EXISTS (
                   SELECT 1 FROM mini_warehouse_assignments a
                   WHERE a.assignment_kind = 'warehouse' AND a.principal_role = $2
                     AND a.principal_ref = $3 AND lower(a.warehouse_name) = lower(w.name)
               )
             ORDER BY i.created_at DESC, i.id DESC",
        )
        .bind(code)
        .bind(role_code(actor)?)
        .bind(&actor.ref_)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn issue_raw_material_paddon(
        &self,
        inventory_actor: &InventoryActor,
        mut input: PaddonIssueCreate,
    ) -> Result<Value, PreparationError> {
        let actor = &inventory_actor.principal;
        let weights = input.quantities()?;
        let remainder = input.remainder_kg.as_deref().map(decimal).transpose()?;
        let expected = input.expected_open_kg.as_deref().map(decimal).transpose()?;
        input.code = paddon_code(&input.code)?;
        let request = json!(&input);
        let mut tx = self.pool.begin().await?;
        lock_paddon(&mut tx, &input.code).await?;
        let (paddon_id, warehouse_id, warehouse) = scoped_paddon(&mut tx, actor, &input.code).await?;
        if let Some((saved, response)) = sqlx::query_as::<_, (Value, Value)>(
            "SELECT request_json, result FROM mini_raw_material_paddon_issue_history
             WHERE paddon_id = $1 AND request_id = $2",
        )
        .bind(&paddon_id)
        .bind(&input.request_id)
        .fetch_optional(&mut *tx)
        .await?
        {
            if saved != request {
                return Err(PreparationError::Conflict(
                    "Bu chiqim kaliti avval ishlatilgan",
                ));
            }
            return Ok(response);
        }

        let destination_name = match &input.destination {
            None => warehouse.clone(),
            Some(Destination::Warehouse(id)) => sqlx::query_scalar(
                "SELECT name FROM mini_warehouses WHERE id = $1 AND NOT is_group",
            )
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(PreparationError::Invalid("Qabul qiluvchi ombor topilmadi"))?,
            Some(Destination::State(id)) => {
                if actor.role != PrincipalRole::MaterialTaminotchi {
                    return Err(PreparationError::Forbidden);
                }
                sqlx::query_scalar(
                    "SELECT name FROM mini_inventory_locations
                     WHERE id = $1 AND kind = 'state' AND active FOR UPDATE",
                )
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(PreparationError::Invalid("Faol state topilmadi"))?
            }
        };
        let candidates: Vec<IssueUnit> = sqlx::query_as(&paddon_unit_stock_sql(
            "SELECT u.unit_no, u.stock_id, u.prepared_roll, COALESCE(s.item_code, u.item_code) AS item_code,
                    COALESCE(NULLIF(s.item_name, ''), NULLIF(u.item_name, ''), s.item_code, u.item_code) AS item_name,
                    u.lot_number, COALESCE(s.width_mm, u.width_mm)::text AS width_mm,
                    COALESCE(s.micron, u.micron)::text AS micron
             FROM mini_raw_material_paddon_units u
             LEFT JOIN mini_raw_material_stock s ON s.id = u.stock_id
             WHERE u.paddon_id = $1 AND u.line_no = $2 AND u.issue_id IS NULL
               AND (($5::integer IS NULL AND NOT u.opened) OR (u.opened AND u.unit_no=$5))
               AND (s.id IS NULL OR (lower(s.warehouse) = lower($3) AND s.qty > 0 AND {PADDON_UNIT_STOCK}))
             ORDER BY u.unit_no LIMIT $4",
        ))
        .bind(&paddon_id)
        .bind(input.line_no)
        .bind(&warehouse)
        .bind(weights.len() as i64)
        .bind(input.open_unit_no)
        .fetch_all(&mut *tx)
        .await?;
        if candidates.len() != weights.len() {
            return Err(PreparationError::Insufficient);
        }
        let existing_qr = candidates.iter().any(|unit| unit.prepared_roll);
        if existing_qr && (weights.len() != 1 || remainder.is_some() || input.moves_stock()) {
            return Err(PreparationError::Invalid(
                "Ishlab chiqarilgan rulonni to‘liq, o‘z omboriga yeching",
            ));
        }

        let issue_id = new_id();
        let mut labels = Vec::with_capacity(weights.len());
        let mut assets = Vec::with_capacity(weights.len());
        for (unit, kg) in candidates.iter().zip(weights) {
            let material = &unit.material;
            let total = kg + remainder.unwrap_or(0);
            // Enforce the NUMERIC(18,6) limit on the combined measured weight.
            decimal(&decimal_text(total))?;
            if expected.is_some_and(|prior| prior != total) {
                return Err(PreparationError::Conflict(
                    "Ochilgan birlikning olingan va qolgan vazni qoldiqqa teng bo‘lishi kerak",
                ));
            }
            let source = match &unit.stock_id {
                Some(id) => Some(id.clone()),
                None if remainder.is_some() => Some(
                    receive_paddon_unit(
                        &mut tx,
                        actor,
                        &paddon_id,
                        &warehouse,
                        material,
                        total,
                        PaddonEntry::Packed,
                    )
                    .await?
                    .id,
                ),
                None => None,
            };
            let stock = if unit.prepared_roll {
                let id = source.as_deref().ok_or(PreparationError::StoreFailed)?;
                release_prepared_roll(&mut tx, &warehouse, id, kg).await?
            } else if let Some(id) = source {
                let existing = weigh_paddon_unit(
                    &mut tx, actor, &warehouse, &paddon_id, &issue_id, &id, material, kg,
                    remainder, expected,
                )
                .await?;
                if remainder.is_some() {
                    sqlx::query("UPDATE mini_raw_material_paddon_units SET opened=true,stock_id=$4,
                        item_code=NULL,unweighed_warehouse_id=NULL WHERE paddon_id=$1 AND line_no=$2 AND unit_no=$3")
                        .bind(&paddon_id).bind(input.line_no).bind(unit.unit_no).bind(&id)
                        .execute(&mut *tx).await?;
                    receive_paddon_unit(
                        &mut tx,
                        actor,
                        &paddon_id,
                        &warehouse,
                        material,
                        kg,
                        PaddonEntry::Portion(&issue_id),
                    )
                    .await?
                } else {
                    existing
                }
            } else {
                receive_paddon_unit(
                    &mut tx,
                    actor,
                    &paddon_id,
                    &warehouse,
                    material,
                    kg,
                    PaddonEntry::Whole(&issue_id),
                )
                .await?
            };
            labels.push(json!({
                "barcode": stock.barcode, "item_code": material.item_code,
                "item_name": material.item_name, "lot_number": material.lot_number,
                "kg": decimal_text(kg), "unit_no":unit.unit_no, "width_mm":material.width_mm, "micron":material.micron,
            }));
            assets.push(InventoryAssetSelector {
                asset_kind: InventoryAssetKind::RawMaterial,
                asset_ref: stock.id,
            });
        }

        let destination_kind = match &input.destination {
            Some(Destination::State(_)) => "state",
            _ => "warehouse",
        };
        let transfer_status = match input.destination {
            Some(Destination::Warehouse(id)) if id != warehouse_id => Some(
                PostgresInventoryMovementStore::create_transfer_assets_tx(
                    &mut tx,
                    inventory_actor,
                    &issue_id,
                    &InventoryTransferCreate {
                        source_warehouse_id: warehouse_id,
                        destination_warehouse_id: id,
                        assets: assets.clone(),
                        note: format!("Paddondan yechib olish {}", input.code),
                        idempotency_key: format!("paddon-issue:{issue_id}"),
                    },
                )
                .await
                .map_err(movement_error)?,
            ),
            Some(Destination::State(id)) => {
                PostgresInventoryMovementStore::relocate_batch_assets_tx(
                    &mut tx,
                    inventory_actor,
                    &InventoryRelocationBatchCreate {
                        assets: assets.clone(),
                        physical_location_id: id,
                        note: format!("Paddondan yechib olish {}", input.code),
                        idempotency_key: format!("paddon-issue:{issue_id}"),
                    },
                )
                .await
                .map_err(movement_error)?;
                None
            }
            _ => None,
        };

        let label_warehouse = if transfer_status == Some(InventoryTransferStatus::Received) {
            &destination_name
        } else {
            &warehouse
        };
        let result = json!({
            "issue_id": issue_id, "paddon_code": input.code, "warehouse": warehouse,
            "destination": destination_name, "destination_kind": destination_kind, "label_warehouse": label_warehouse,
            "transfer_status": transfer_status.map(InventoryTransferStatus::as_str),
            "labels": labels, "remainder_kg": remainder.map(decimal_text), "print_tracking": true,
            "existing_qr": existing_qr,
        });
        sqlx::query(
            "INSERT INTO mini_raw_material_paddon_issues
             (id, paddon_id, request_id, request_json, response_json) VALUES ($1,$2,$3,$4,$5)",
        )
        .bind(&issue_id)
        .bind(&paddon_id)
        .bind(&input.request_id)
        .bind(request)
        .bind(&result)
        .execute(&mut *tx)
        .await?;
        if remainder.is_none() {
            for (unit, asset) in candidates.iter().zip(assets) {
                sqlx::query(
                    "UPDATE mini_raw_material_paddon_units
                 SET issue_id=$4, stock_id=$5, item_code=NULL, unweighed_warehouse_id=NULL
                 WHERE paddon_id=$1 AND line_no=$2 AND unit_no=$3",
                )
                .bind(&paddon_id)
                .bind(input.line_no)
                .bind(unit.unit_no)
                .bind(&issue_id)
                .bind(asset.asset_ref)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok(result)
    }
}

async fn release_prepared_roll(
    tx: &mut Transaction<'_, Postgres>,
    warehouse: &str,
    id: &str,
    expected_kg: i64,
) -> Result<PaddonStock, PreparationError> {
    sqlx::query("SELECT id FROM mini_raw_material_stock WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(PreparationError::Insufficient)?;
    let barcode = sqlx::query_scalar(&format!(
        "UPDATE mini_raw_material_stock s SET status='available', updated_at=now()
         WHERE s.id=$1 AND lower(s.warehouse)=lower($2) AND s.qty=$3::text::numeric
           AND {PREPARED_PADDON_STOCK} RETURNING s.barcode"
    ))
    .bind(id)
    .bind(warehouse)
    .bind(decimal_text(expected_kg))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::Conflict("Rulon qoldig‘i o‘zgargan. Paddonni yangilang"))?;
    Ok(PaddonStock { id: id.into(), barcode })
}

async fn weigh_paddon_unit(
    tx: &mut Transaction<'_, Postgres>,
    actor: &Principal,
    warehouse: &str,
    paddon_id: &str,
    issue_id: &str,
    stock_id: &str,
    material: &PaddonMaterial,
    kg: i64,
    remainder: Option<i64>,
    expected: Option<i64>,
) -> Result<PaddonStock, PreparationError> {
    let (barcode, available): (String, String) = sqlx::query_as(
        "SELECT barcode, qty::text FROM mini_raw_material_stock WHERE id=$1 FOR UPDATE",
    )
    .bind(stock_id)
    .fetch_one(&mut **tx)
    .await?;
    // Recheck warehouse, assignments and transfers after taking the stock lock.
    if !sqlx::query_scalar::<_, bool>(&format!(
        "SELECT EXISTS (SELECT 1 FROM mini_raw_material_stock s
         WHERE s.id=$1 AND lower(s.warehouse)=lower($2) AND s.qty>0 AND {PADDON_STOCK})"
    ))
    .bind(stock_id)
    .bind(warehouse)
    .fetch_one(&mut **tx)
    .await?
    {
        return Err(PreparationError::Insufficient);
    }
    let previous_kg = decimal(&available)?;
    if expected.is_some_and(|value| value != previous_kg) {
        return Err(PreparationError::Conflict(
            "Qoldiq o‘zgargan. Paddondagi ma’lumotni yangilang",
        ));
    }
    sqlx::query(
        "UPDATE mini_raw_material_stock SET qty=$2::text::numeric, updated_at=now(), status=$5,
         payload_json=payload_json || jsonb_build_object('paddon_issue_id',$3::text,'lot_number',$4::text)
         WHERE id=$1",
    )
    .bind(stock_id)
    .bind(decimal_text(remainder.unwrap_or(kg)))
    .bind(issue_id)
    .bind(&material.lot_number)
    .bind(if remainder.is_some() { "on_paddon" } else { "available" })
    .execute(&mut **tx)
    .await?;
    record_paddon_event(
        tx,
        actor,
        warehouse,
        material,
        &barcode,
        kg + remainder.unwrap_or(0) - previous_kg,
        PaddonEvent::Weighed {
            previous: previous_kg,
            packed: remainder.is_some(),
        },
        issue_id,
        paddon_id,
    )
    .await?;
    if remainder.is_some() {
        record_paddon_event(
            tx,
            actor,
            warehouse,
            material,
            &barcode,
            -kg,
            PaddonEvent::SplitOut,
            issue_id,
            paddon_id,
        )
        .await?;
    }
    Ok(PaddonStock {
        id: stock_id.into(),
        barcode,
    })
}

fn movement_error(error: InventoryMovementError) -> PreparationError {
    match error {
        InventoryMovementError::WarehouseForbidden => PreparationError::Forbidden,
        InventoryMovementError::MaterialWarehouseScopeMissing => {
            PreparationError::MaterialNotInWarehouse
        }
        InventoryMovementError::StoreFailed => PreparationError::StoreFailed,
        InventoryMovementError::AssetUnavailable
        | InventoryMovementError::AssetNotInSourceWarehouse => {
            PreparationError::Conflict("Xomashyoning joyi yoki qoldig‘i o‘zgardi")
        }
        _ => PreparationError::Conflict("Tanlangan manzilga ko‘chirib bo‘lmadi"),
    }
}
